use std::{
    env, fs,
    path::{Path, PathBuf},
};
fn files(root: &Path, dir: &Path, out: &mut String) {
    for entry in fs::read_dir(dir).expect("read desktop web assets") {
        let path = entry.expect("web entry").path();
        if path.is_dir() {
            files(root, &path, out);
        } else {
            let relative = path
                .strip_prefix(root)
                .unwrap()
                .to_str()
                .unwrap()
                .replace('\\', "/");
            assert!(
                fs::metadata(&path).unwrap().len() <= 8 * 1024 * 1024,
                "asset exceeds bound"
            );
            out.push_str(&format!(
                "({relative:?}, include_bytes!({:?})),\n",
                path.to_str().unwrap()
            ));
        }
    }
}
fn main() {
    println!("cargo:rerun-if-env-changed=ORVEXA_DESKTOP_WEB_DIR");
    let mut source = String::from("pub static ASSETS: &[(&str, &[u8])] = &[\n");
    if let Some(path) = env::var_os("ORVEXA_DESKTOP_WEB_DIR") {
        let path = PathBuf::from(path)
            .canonicalize()
            .expect("built web asset directory");
        assert!(
            path.join("index.html").is_file(),
            "build web before packaging app"
        );
        println!("cargo:rerun-if-changed={}", path.display());
        files(&path, &path, &mut source);
    }
    source.push_str("];\n");
    fs::write(
        PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("assets.rs"),
        source,
    )
    .unwrap();
}
