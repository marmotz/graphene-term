// Embeds the build version as `GRAPHENE_VERSION`: `<pkg>-dev.<UTC YYYYMMDDHHMMSS>` by
// default, `<pkg>` when built with `GRAPHENE_CHANNEL=stable`. The timestamp comes from
// `SOURCE_DATE_EPOCH` when set, else the current time. The channel is embedded as
// `GRAPHENE_BUILD_CHANNEL` (`dev` or `stable`).

use std::time::{SystemTime, UNIX_EPOCH};

include!("src/build_version.rs");

fn main() {
    println!("cargo:rerun-if-env-changed=SOURCE_DATE_EPOCH");
    println!("cargo:rerun-if-env-changed=GRAPHENE_CHANNEL");
    println!("cargo:rerun-if-changed=src");
    println!("cargo:rerun-if-changed=Cargo.toml");

    let channel = std::env::var("GRAPHENE_CHANNEL").ok();
    let channel = BuildChannel::parse(channel.as_deref()).unwrap_or_else(|e| panic!("{e}"));

    let epoch = match std::env::var("SOURCE_DATE_EPOCH") {
        Ok(value) => value
            .trim()
            .parse::<u64>()
            .unwrap_or_else(|_| panic!("invalid SOURCE_DATE_EPOCH {value:?}")),
        Err(_) => SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock before 1970")
            .as_secs(),
    };

    let pkg_version = std::env::var("CARGO_PKG_VERSION").expect("CARGO_PKG_VERSION");
    let version = build_version(&pkg_version, channel, epoch);
    println!("cargo:rustc-env=GRAPHENE_VERSION={version}");
    let channel = match channel {
        BuildChannel::Dev => "dev",
        BuildChannel::Stable => "stable",
    };
    println!("cargo:rustc-env=GRAPHENE_BUILD_CHANNEL={channel}");
}
