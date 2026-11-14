# Graphene shell integration for fish.
#
# Graphene prepends the parent of this folder's `fish` directory to XDG_DATA_DIRS, so
# fish sources this file as vendor configuration. It makes the interactive shell tell
# Graphene when a command starts and ends; fish's own OSC 133 marks are ignored. The
# prompt is never read, wrapped or rebuilt, and the user's dotfiles are never modified.

if set -q GRAPHENE_XDG_DATA_DIRS
    set -gx XDG_DATA_DIRS $GRAPHENE_XDG_DATA_DIRS
    set -e GRAPHENE_XDG_DATA_DIRS
else
    set -e XDG_DATA_DIRS
end

# The session token stays in a shell variable: commands run from the shell never see it.
if set -q GRAPHENE_BLOCK_TOKEN
    set -g __graphene_token $GRAPHENE_BLOCK_TOKEN
    set -e GRAPHENE_BLOCK_TOKEN
end

if status is-interactive; and test -n "$__graphene_token"
    # $argv is the command line. Not emitted for an empty one.
    function __graphene_preexec --on-event fish_preexec
        set -l encoded (printf '%s' $argv | command base64 -w0 2>/dev/null)
        printf '\e]6973;%s;start;%s\e\\' $__graphene_token "$encoded"
    end

    function __graphene_postexec --on-event fish_postexec
        printf '\e]6973;%s;end;%s\e\\' $__graphene_token $status
    end

    # The line editor is about to read a command line: the rich input editor of
    # Graphene may send its key sequences.
    function __graphene_prompt --on-event fish_prompt
        printf '\e]6973;%s;prompt\e\\' $__graphene_token
    end

    # Rich input editor: Graphene sends private sequences (CSI <code> u, code points
    # from the Unicode private use area) for the editor moves, mirrored from
    # src/input_editor.rs. Every move is passive: clamped at the buffer ends and never
    # accepting the autosuggestion.

    # Runs a forward input function only when the cursor is not at the end of the
    # buffer, the only place the autosuggestion shows: there forward-word, kill-word,
    # end-of-line and end-of-buffer would accept it.
    function __graphene_forward --argument-names input_function
        set -l pos (commandline -C)
        commandline -C 2147483647
        set -l last (commandline -C)
        commandline -C $pos
        if test $pos -lt $last
            commandline -f $input_function
        end
    end

    function __graphene_bind
        # The -passive functions come with fish 4.0.
        set -l left backward-char
        set -l right '__graphene_forward forward-char'
        if contains -- backward-char-passive (bind --function-names)
            set left backward-char-passive
            set right forward-char-passive
        end
        # Raw sequences starting with \e keep the old bind syntax in fish 4.
        for mode in default insert
            bind -M $mode \e\[59136u $left
            bind -M $mode \e\[59137u $right
            bind -M $mode \e\[59138u backward-word
            # forward-word stops at the word end: forward-word-vi reaches the start of
            # the next word.
            bind -M $mode \e\[59139u '__graphene_forward forward-word-vi'
            bind -M $mode \e\[59140u backward-kill-word
            bind -M $mode \e\[59141u '__graphene_forward kill-word'
            bind -M $mode \e\[59142u beginning-of-line
            bind -M $mode \e\[59143u '__graphene_forward end-of-line'
            bind -M $mode \e\[59144u up-line
            bind -M $mode \e\[59145u down-line
            bind -M $mode \e\[59146u beginning-of-buffer
            bind -M $mode \e\[59147u '__graphene_forward end-of-buffer'
            bind -M $mode \e\[59148u 'commandline -i \n'
            # forward-word already stops at the end of the word (forward-word-end
            # stops on its last character).
            bind -M $mode \e\[59149u '__graphene_forward forward-word'
        end
    end

    # Before each prompt, after the user's configuration: switching key bindings
    # (fish_vi_key_bindings, fish_key_bindings) erases them, so bind again when gone.
    function __graphene_rebind --on-event fish_prompt
        if not bind -M default \e\[59148u >/dev/null 2>&1
            __graphene_bind
        end
    end
end
