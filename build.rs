use std::path::PathBuf;
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=icon.png");
    println!("cargo:rerun-if-changed=build.rs");

    let target_os = std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    if target_os != "windows" {
        return;
    }

    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let png = manifest.join("icon.png");
    if !png.is_file() {
        println!("cargo:warning=icon.png missing, skipping .exe icon embed");
        return;
    }

    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap());
    let ico = out.join("icon.ico");
    if let Err(err) = build_ico(&png, &ico) {
        println!("cargo:warning=could not build icon.ico: {err}");
        return;
    }

    let rc = out.join("icon.rc");
    let ico_ref = ico.to_string_lossy().replace('\\', "/");
    if let Err(err) = std::fs::write(&rc, format!("1 ICON \"{}\"\n", ico_ref)) {
        println!("cargo:warning=could not write icon.rc: {err}");
        return;
    }

    let host_windows = std::env::var("HOST")
        .map(|h| h.contains("windows"))
        .unwrap_or(false);
    let windres = if host_windows {
        "windres".to_string()
    } else {
        "x86_64-w64-mingw32-windres".to_string()
    };

    let obj = out.join("icon_res.o");
    let status = Command::new(&windres)
        .arg("-I")
        .arg(&out)
        .arg(&rc)
        .arg("-O")
        .arg("coff")
        .arg("-o")
        .arg(&obj)
        .status();

    match status {
        Ok(s) if s.success() => {
            println!("cargo:rustc-link-arg={}", obj.to_string_lossy());
        }
        Ok(s) => println!("cargo:warning=windres exited with {s}"),
        Err(err) => println!("cargo:warning=failed to run {windres}: {err}"),
    }
}

fn build_ico(png: &std::path::Path, ico: &std::path::Path) -> Result<(), Box<dyn std::error::Error>> {
    use std::fs::File;
    use std::io::BufWriter;

    let img = image::open(png)?.to_rgba8();
    let sizes = [16u32, 24, 32, 48, 64, 128, 256];

    let mut dir = ico::IconDir::new(ico::ResourceType::Icon);
    for size in sizes {
        let resized = image::imageops::resize(&img, size, size, image::imageops::FilterType::Lanczos3);
        let raw = resized.into_raw();
        let entry = ico::IconImage::from_rgba_data(size, size, raw);
        dir.add_entry(ico::IconDirEntry::encode(&entry)?);
    }
    let writer = BufWriter::new(File::create(ico)?);
    dir.write(writer)?;
    Ok(())
}
