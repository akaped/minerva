//! Embeds the application icon and version information in the Windows executable.
use std::path::Path;

fn main() {
    println!("cargo:rerun-if-changed=assets/minerva.ico");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let out = std::env::var("OUT_DIR").unwrap();
    let ico = Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/minerva.ico");

    let version = env!("CARGO_PKG_VERSION");
    let numbers: Vec<&str> = version.split(['.', '-']).take(3).collect();
    let numeric = format!("{},0", numbers.join(","));
    let rc = Path::new(&out).join("minerva.rc");
    std::fs::write(
        &rc,
        format!(
            r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "CompanyName", "Minerva"
      VALUE "FileDescription", "Minerva"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "minerva"
      VALUE "OriginalFilename", "minerva.exe"
      VALUE "ProductName", "Minerva"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
            icon = ico.display().to_string().replace('\\', "/"),
        ),
    )
    .unwrap();
    embed_resource::compile(&rc, embed_resource::NONE)
        .manifest_optional()
        .unwrap();
}
