//! Transmission media of the image data: direct, file, temporary file or shared memory.

use super::*;

/// Directories a file medium (`t=f`) may not read from.
pub(super) const FORBIDDEN_DIRS: [&str; 3] = ["/proc", "/sys", "/dev"];

/// Part a temporary file medium (`t=t`) must have in its path, as the protocol requires.
pub(super) const TEMP_MARKER: &str = "tty-graphics-protocol";

/// The bytes of the image a command transmits: its base64 `data` decoded, or the file or
/// shared memory it names read.
pub fn load(command: &Command, data: &[u8]) -> Result<Vec<u8>, Error> {
    let decoded = BASE64
        .decode(data)
        .map_err(|err| Error::invalid(format!("bad base64 data: {err}")))?;
    if command.medium == b'd' {
        return Ok(decoded);
    }
    let name =
        String::from_utf8(decoded).map_err(|_| Error::invalid("the file name is not UTF-8"))?;
    match command.medium {
        b'f' => read_file(Path::new(&name), command),
        b't' => read_temp_file(Path::new(&name), command),
        b's' => read_shared_memory(&name, command),
        other => Err(Error::invalid(format!(
            "unknown transmission medium: {}",
            other as char
        ))),
    }
}

pub(super) fn bad_file(path: &Path, err: impl fmt::Display) -> Error {
    Error::new("EBADF", format!("{}: {err}", path.display()))
}

/// Opens `path` when it is a regular file. Non-blocking, so a FIFO named by a client
/// is refused instead of blocking until a writer appears.
pub(super) fn open_regular(path: &Path) -> Result<File, Error> {
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|err| bad_file(path, err))?;
    let metadata = file.metadata().map_err(|err| bad_file(path, err))?;
    if !metadata.is_file() {
        return Err(bad_file(path, "not a regular file"));
    }
    Ok(file)
}

pub(super) fn read_file(path: &Path, command: &Command) -> Result<Vec<u8>, Error> {
    let real = path.canonicalize().map_err(|err| bad_file(path, err))?;
    if FORBIDDEN_DIRS.iter().any(|dir| real.starts_with(dir)) {
        return Err(bad_file(path, "reading from this directory is not allowed"));
    }
    read_from(open_regular(&real)?, command)
}

/// Reads a temporary file and deletes it, as the protocol requires. Only files under a
/// temporary directory, with [`TEMP_MARKER`] in their path, are read or deleted.
pub(super) fn read_temp_file(path: &Path, command: &Command) -> Result<Vec<u8>, Error> {
    let real = path.canonicalize().map_err(|err| bad_file(path, err))?;
    let in_temp = temp_dirs().iter().any(|dir| real.starts_with(dir));
    if !in_temp || !real.to_string_lossy().contains(TEMP_MARKER) {
        return Err(bad_file(
            path,
            "not a temporary file of the graphics protocol",
        ));
    }
    let bytes = open_regular(&real).and_then(|file| read_from(file, command));
    if let Err(err) = std::fs::remove_file(&real) {
        debug!("cannot delete {}: {err}", real.display());
    }
    bytes
}

pub(super) fn temp_dirs() -> Vec<PathBuf> {
    [
        std::env::temp_dir(),
        PathBuf::from("/tmp"),
        PathBuf::from("/dev/shm"),
    ]
    .into_iter()
    .filter_map(|dir| dir.canonicalize().ok())
    .collect()
}

/// Reads a POSIX shared memory object and unlinks it, as the protocol requires.
pub(super) fn read_shared_memory(name: &str, command: &Command) -> Result<Vec<u8>, Error> {
    let bad = |err: &dyn fmt::Display| Error::new("EBADF", format!("{name}: {err}"));
    let c_name = CString::new(name).map_err(|err| bad(&err))?;
    // SAFETY: `c_name` is a valid NUL-terminated string.
    let fd = unsafe { libc::shm_open(c_name.as_ptr(), libc::O_RDONLY, 0) };
    if fd < 0 {
        return Err(bad(&std::io::Error::last_os_error()));
    }
    #[cfg(target_os = "macos")]
    let bytes = read_shared_memory_mapping(fd, command);
    #[cfg(not(target_os = "macos"))]
    let bytes = {
        // SAFETY: `fd` was just opened and is owned by nothing else.
        let file = unsafe { File::from_raw_fd(fd) };
        read_from(file, command)
    };
    #[cfg(target_os = "macos")]
    // SAFETY: the mapping helper borrows `fd` and releases its mapping before returning.
    unsafe {
        libc::close(fd);
    }
    // SAFETY: `c_name` is a valid NUL-terminated string.
    unsafe { libc::shm_unlink(c_name.as_ptr()) };
    bytes
}

/// macOS shared-memory descriptors support mapping but not ordinary `read` calls.
#[cfg(target_os = "macos")]
pub(super) fn read_shared_memory_mapping(
    fd: libc::c_int,
    command: &Command,
) -> Result<Vec<u8>, Error> {
    let bad = |err: &dyn fmt::Display| Error::new("EBADF", format!("shared memory: {err}"));
    // SAFETY: `stat` is a plain C struct and `fstat` initializes it on success.
    let mut stat: libc::stat = unsafe { std::mem::zeroed() };
    if unsafe { libc::fstat(fd, &mut stat) } != 0 {
        return Err(bad(&std::io::Error::last_os_error()));
    }
    let length = usize::try_from(stat.st_size).map_err(|err| bad(&err))?;
    let offset = usize::try_from(command.offset).map_err(|err| bad(&err))?;
    if offset >= length {
        return Ok(Vec::new());
    }
    let limit = match command.size {
        0 => MAX_DATA + 1,
        size => usize::try_from(size).map_err(|err| bad(&err))?,
    };
    let count = (length - offset).min(limit);
    if count > MAX_DATA {
        return Err(Error::too_big());
    }
    if count == 0 {
        return Ok(Vec::new());
    }

    // mmap requires a page-aligned offset; only map the requested range and its prefix.
    // SAFETY: `sysconf` has no memory preconditions.
    let page_size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if page_size <= 0 {
        return Err(bad(&std::io::Error::last_os_error()));
    }
    let page_size = page_size as usize;
    let map_offset = offset / page_size * page_size;
    let prefix = offset - map_offset;
    let map_length = prefix.checked_add(count).ok_or_else(Error::too_big)?;
    // SAFETY: the descriptor is open, the offset is page-aligned, and `map_length`
    // stays within the shared-memory object as checked above.
    let mapping = unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            map_length,
            libc::PROT_READ,
            libc::MAP_SHARED,
            fd,
            map_offset as libc::off_t,
        )
    };
    if mapping == libc::MAP_FAILED {
        return Err(bad(&std::io::Error::last_os_error()));
    }
    // SAFETY: the mapping covers `prefix + count` readable bytes.
    let bytes =
        unsafe { std::slice::from_raw_parts(mapping.cast::<u8>().add(prefix), count) }.to_vec();
    // SAFETY: `mapping` was returned by mmap with this exact length.
    unsafe {
        libc::munmap(mapping, map_length);
    }
    Ok(bytes)
}

/// Reads `command.size` bytes (all when 0) from `command.offset` on.
pub(super) fn read_from(mut file: File, command: &Command) -> Result<Vec<u8>, Error> {
    let fail = |err: std::io::Error| Error::new("EBADF", err.to_string());
    if command.offset > 0 {
        file.seek(SeekFrom::Start(u64::from(command.offset)))
            .map_err(fail)?;
    }
    let limit = match command.size {
        0 => MAX_DATA as u64 + 1,
        size => u64::from(size),
    };
    let mut bytes = Vec::new();
    file.take(limit).read_to_end(&mut bytes).map_err(fail)?;
    if bytes.len() > MAX_DATA {
        return Err(Error::too_big());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_data_is_base64_with_or_without_padding() {
        assert_eq!(load(&command("t=d"), b"AQI=").unwrap(), [1, 2]);
        assert_eq!(load(&command("t=d"), b"AQI").unwrap(), [1, 2]);
        assert_eq!(load(&command("t=d"), b"*").unwrap_err().code, "EINVAL");
        assert_eq!(load(&command("t=x"), b"AQI=").unwrap_err().code, "EINVAL");
    }

    /// A unique path in the temporary directory, with the protocol marker.
    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "graphene-{}-{TEMP_MARKER}-{name}",
            std::process::id()
        ))
    }

    fn encoded(path: &Path) -> Vec<u8> {
        BASE64
            .encode(path.to_str().unwrap().as_bytes())
            .into_bytes()
    }

    #[test]
    fn file_medium_reads_a_range_and_keeps_the_file() {
        let path = temp_path("file");
        std::fs::write(&path, b"0123456789").unwrap();
        assert_eq!(
            load(&command("t=f"), &encoded(&path)).unwrap(),
            b"0123456789"
        );
        assert_eq!(
            load(&command("t=f,O=2,S=3"), &encoded(&path)).unwrap(),
            b"234"
        );
        assert!(path.exists());
        std::fs::remove_file(&path).unwrap();

        let err = load(&command("t=f"), &encoded(&path)).unwrap_err();
        assert_eq!(err.code, "EBADF");
        let err = load(&command("t=f"), &encoded(Path::new("/proc/self/status"))).unwrap_err();
        assert_eq!(err.code, "EBADF");
        let err = load(&command("t=f"), &encoded(&std::env::temp_dir())).unwrap_err();
        assert_eq!(err.code, "EBADF");
    }

    #[test]
    fn file_medium_refuses_a_fifo_without_blocking() {
        let path = temp_path("fifo");
        let c_path = CString::new(path.to_str().unwrap()).unwrap();
        // SAFETY: valid NUL-terminated path.
        assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
        let err = load(&command("t=f"), &encoded(&path)).unwrap_err();
        assert_eq!(err.code, "EBADF");
        std::fs::remove_file(&path).unwrap();
    }

    #[test]
    fn temporary_file_medium_deletes_the_file_after_reading() {
        let path = temp_path("temp");
        std::fs::write(&path, b"data").unwrap();
        assert_eq!(load(&command("t=t"), &encoded(&path)).unwrap(), b"data");
        assert!(!path.exists());

        // Without the protocol marker, the file is neither read nor deleted.
        let plain = std::env::temp_dir().join(format!("graphene-{}-plain", std::process::id()));
        std::fs::write(&plain, b"data").unwrap();
        let err = load(&command("t=t"), &encoded(&plain)).unwrap_err();
        assert_eq!(err.code, "EBADF");
        assert!(plain.exists());
        std::fs::remove_file(&plain).unwrap();
    }

    #[test]
    fn shared_memory_medium_reads_and_unlinks_the_object() {
        let name = format!("/graphene-test-{}", std::process::id());
        let c_name = CString::new(name.clone()).unwrap();
        // SAFETY: valid NUL-terminated shared-memory name.
        let fd = unsafe {
            libc::shm_open(
                c_name.as_ptr(),
                libc::O_CREAT | libc::O_EXCL | libc::O_RDWR,
                0o600,
            )
        };
        assert!(fd >= 0, "{}", std::io::Error::last_os_error());
        let bytes = b"shared";
        // POSIX shared memory is created with length zero on macOS; size it before writing.
        // SAFETY: `fd` is a live shared-memory descriptor and the requested size is small.
        let result = unsafe { libc::ftruncate(fd, bytes.len() as libc::off_t) };
        assert_eq!(result, 0, "{}", std::io::Error::last_os_error());
        // SAFETY: `fd` was just opened and the requested range has been allocated.
        let mapping = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                bytes.len(),
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        assert_ne!(
            mapping,
            libc::MAP_FAILED,
            "{}",
            std::io::Error::last_os_error()
        );
        // SAFETY: the mapping is writable for `bytes.len()` bytes and the source is valid.
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapping.cast(), bytes.len());
            libc::munmap(mapping, bytes.len());
            libc::close(fd);
        }

        let data = BASE64.encode(name.as_bytes());
        assert_eq!(
            load(&command("t=s,O=1,S=3"), data.as_bytes()).unwrap(),
            b"har"
        );
        // Unlinked: a second read fails.
        let err = load(&command("t=s"), data.as_bytes()).unwrap_err();
        assert_eq!(err.code, "EBADF");
    }

    fn command(control: &str) -> Command {
        parse(control.as_bytes()).unwrap().0
    }
}
