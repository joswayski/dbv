// Embeds the Satoshi variable font when `npm run fonts` has fetched it.
//
// The ITF Free Font License allows embedding the font in applications but not
// redistributing the files, so `assets/fonts` is git-ignored and this build
// falls back to the system font when it is absent.

use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=../../assets/fonts/Satoshi-Variable.ttf");
    let font = Path::new("../../assets/fonts/Satoshi-Variable.ttf");
    let out = std::env::var("OUT_DIR").expect("OUT_DIR");
    let generated = if font.exists() {
        format!(
            "pub const SATOSHI: Option<&[u8]> = Some(include_bytes!({:?}));\n",
            font.canonicalize().expect("font path").to_string_lossy()
        )
    } else {
        "pub const SATOSHI: Option<&[u8]> = None;\n".to_owned()
    };
    std::fs::write(Path::new(&out).join("satoshi.rs"), generated).expect("write satoshi.rs");
}
