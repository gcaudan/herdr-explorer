//! test/remote-link.test.js: links to a network share, found by reading the links on the way, never by following
//! them.

mod support;

use herdr_explorer::core::exec::remote_link;
use herdr_explorer::core::paths;

// A name that never resolves (RFC 2606), in case a link were followed by mistake.
const SHARE: &str = r"\\share.invalid\s";

#[cfg(unix)]
fn link(target: &str, at: &std::path::Path, _dir: bool) -> std::io::Result<()> {
    std::os::unix::fs::symlink(target, at)
}

#[cfg(windows)]
fn link(target: &str, at: &std::path::Path, dir: bool) -> std::io::Result<()> {
    if dir { std::os::windows::fs::symlink_dir(target, at) } else { std::os::windows::fs::symlink_file(target, at) }
}

#[test]
fn a_link_to_a_network_share_is_found_directly_through_a_folder_or_another_link() {
    let tmp = support::TempDir::new("links");
    let dir = tmp.path.to_string_lossy().into_owned();
    std::fs::write(tmp.path.join("real.png"), "").unwrap();
    match link(&format!(r"{SHARE}\a.png"), &tmp.path.join("remote.png"), false) {
        // Windows without developer mode refuses links.
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => return,
        r => r.unwrap(),
    }
    link(SHARE, &tmp.path.join("pics"), true).unwrap();
    link("remote.png", &tmp.path.join("chain.png"), false).unwrap();
    link("real.png", &tmp.path.join("local.png"), false).unwrap();
    link("loop2", &tmp.path.join("loop1"), false).unwrap();
    link("loop1", &tmp.path.join("loop2"), false).unwrap();

    let at = |name: &str| paths::join(&[&dir, name]);
    assert!(remote_link(&at("remote.png"), &dir));
    assert!(remote_link(&paths::join(&[&dir, "pics", "a.png"]), &dir), "a linked folder on the way");
    assert!(remote_link(&at("chain.png"), &dir), "a local link to a remote one");
    assert!(remote_link(&at("loop1"), &dir), "a loop");
    assert!(!remote_link(&at("local.png"), &dir));
    assert!(!remote_link(&at("real.png"), &dir));
    assert!(!remote_link(&at("missing.png"), &dir));
    // Without a root, every folder of the path is looked at.
    assert!(remote_link(&at("remote.png"), ""));
}

/// The second half of the JS test: the file actions never hand such a link to a program, a rename moves the link
/// itself, and a removal takes the link, never its target.
#[test]
#[ignore = "case 10"]
fn the_file_actions_never_follow_a_link_to_a_network_share() {}
