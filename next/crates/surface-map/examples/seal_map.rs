use itasha_surface_map::PaintSurfaceMap;
use std::io::Write;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<_> = std::env::args().collect();
    let mut map: PaintSurfaceMap =
        serde_json::from_slice(&std::fs::read(args.get(1).ok_or("input required")?)?)?;
    // Reimported float32 coordinates and stable identities define canonical bytes.
    map.paint_fingerprint = map.fingerprint();
    map.validate()?;
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(args.get(2).ok_or("new output required")?)?;
    file.write_all(&serde_json::to_vec(&map)?)?;
    eprintln!(
        "Sealed {} triangles {}",
        map.triangles.len(),
        map.paint_fingerprint
    );
    Ok(())
}
