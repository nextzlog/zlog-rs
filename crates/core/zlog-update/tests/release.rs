const RELEASE: &str = include_str!("../../../../scripts/release.sh");
const PUBLISH: &str = include_str!("../../../../scripts/publish.sh");
const BUILD: &str = include_str!("../../../../.github/workflows/build.yaml");

#[test]
fn version_and_tag_move_only_after_every_platform_built() {
    assert!(!RELEASE.contains("version.txt"));
    assert!(!RELEASE.contains("git/refs/tags"));
    assert!(
        !RELEASE.contains("gh release"),
        "builds never touch the release"
    );
    assert!(RELEASE.contains("assets/"));
    assert!(PUBLISH.contains("> assets/version.txt"));
    assert!(PUBLISH.contains("refs/heads/master"));
    let upload = PUBLISH
        .find("gh release upload \"$TAG\" assets/*")
        .expect("one upload");
    let tag = PUBLISH.find("git/refs/tags/$TAG").expect("tag move");
    assert!(upload < tag, "the tag moves after the upload");
    assert_eq!(PUBLISH.matches("gh release upload").count(), 1);
    let publish = &BUILD[BUILD.find("  publish:").expect("publish job")..];
    assert!(publish.contains("needs: build"));
    assert!(publish.contains("if: success() && github.ref == 'refs/heads/master'"));
    assert!(publish.contains("scripts/publish.sh"));
    assert!(BUILD.contains("actions/upload-artifact@v4"));
    assert!(BUILD.contains("name: assets-${{ runner.os }}"));
    assert!(BUILD.contains("actions/download-artifact@v4"));
    assert!(BUILD.contains("merge-multiple: true"));
    assert!(!BUILD.contains('#'));
}

#[test]
fn ci_downloads_are_cached_checked_and_retried() {
    assert!(BUILD.contains("cargo-about-0.9.2"));
    assert!(RELEASE.contains("command -v cargo-about"));
    assert!(RELEASE.contains("177ELF"));
    assert!(RELEASE.contains("for n in 1 2 3"));
}
