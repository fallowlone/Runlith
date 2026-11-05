use std::path::{Path, PathBuf};

pub fn binary(root: &Path, name: &str) -> PathBuf {
    root.join("installed").join(name)
}

pub fn install(root: &Path) {
    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(root.join("installed"))
        .expect("private test installation");
    let mut profile = std::env::current_exe().expect("test executable");
    profile.pop();
    profile.pop();
    for (source, name) in [
        (profile.join("agentctl"), "agentctl"),
        (profile.join("examples/confirmed-daemon-fixture"), "agentd"),
    ] {
        assert!(
            source.is_file(),
            "build agentctl and --example confirmed-daemon-fixture first: {source:?}"
        );
        let target = binary(root, name);
        if std::fs::hard_link(&source, &target).is_err() {
            std::fs::copy(&source, &target).expect("install test executable");
        }
    }
    std::fs::File::create(root.join("installed/.confirmed-test-daemon"))
        .expect("explicit test-daemon marker");
}
