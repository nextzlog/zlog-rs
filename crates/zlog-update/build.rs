//! Embeds the identity of the build: the commit it was built from (`GITHUB_SHA`,
//! set by GitHub Actions) and the time of the build, both in UTC.
//! Local builds have no commit and report themselves as development builds.

use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    println!("cargo:rerun-if-env-changed=GITHUB_SHA");
    println!("cargo:rerun-if-changed=build.rs");
    let commit = std::env::var("GITHUB_SHA").unwrap_or_default();
    println!("cargo:rustc-env=ZLOG_COMMIT={}", commit.trim());
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    println!("cargo:rustc-env=ZLOG_BUILT={}", utc(secs));
}

/// `secs` since 1970 as `YYYY-MM-DD hh:mm UTC` (civil-from-days of H. Hinnant).
fn utc(secs: u64) -> String {
    let days = (secs / 86400) as i64;
    let rem = secs % 86400;
    let z = days + 719468;
    let era = z.div_euclid(146097);
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02} {:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60
    )
}
