//! render::image::convert: a WebP turned into a PNG by dwebp, here a fake one first on a PATH of its own.

#[cfg(unix)]
#[test]
fn a_webp_goes_through_dwebp_when_it_is_installed() {
    use herdr_explorer::render::image::convert;
    use std::os::unix::fs::PermissionsExt;

    let dir = std::env::temp_dir().join(format!("herdr-explorer-dwebp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let dwebp = dir.join("dwebp");
    let args = dir.join("args");
    let fake = |body: &str| {
        std::fs::write(&dwebp, format!("#!/bin/sh\nprintf '%s\\n' \"$@\" > '{}'\n{body}\n", args.display())).unwrap();
        std::fs::set_permissions(&dwebp, std::fs::Permissions::from_mode(0o755)).unwrap();
    };
    let path = std::env::var_os("PATH");
    // SAFETY: the only test of this binary, no other thread reads the environment.
    unsafe { std::env::set_var("PATH", &dir) };

    let mut webp = b"RIFF\x20\0\0\0WEBPVP8 ".to_vec();
    webp.extend([0u8; 20]);
    let png = b"\x89PNG\r\n\x1a\nrest";
    fake(r"printf '\211PNG\r\n\032\nrest'");
    assert_eq!(convert("/x/a.webp", &webp).as_deref(), Some(&png[..]));
    assert_eq!(std::fs::read_to_string(&args).unwrap(), "-quiet\n/x/a.webp\n-o\n-\n");
    // Not a WebP, or a relative path (it could read as an option): dwebp is not run.
    std::fs::remove_file(&args).unwrap();
    assert_eq!(convert("/x/a.webp", b"\x89PNG\r\n\x1a\n0000000000"), None);
    assert_eq!(convert("-a.webp", &webp), None);
    assert!(!args.exists());
    // dwebp failing, or writing something other than a PNG.
    fake(r"printf '\211PNG\r\n\032\nrest'; exit 1");
    assert_eq!(convert("/x/a.webp", &webp), None);
    fake("printf 'GIF89a'");
    assert_eq!(convert("/x/a.webp", &webp), None);
    // No dwebp.
    std::fs::remove_file(&dwebp).unwrap();
    assert_eq!(convert("/x/a.webp", &webp), None);

    match path {
        Some(p) => unsafe { std::env::set_var("PATH", p) },
        None => unsafe { std::env::remove_var("PATH") },
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
