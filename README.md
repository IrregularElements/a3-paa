`a3-paa`: Bohemia Interactive's PAA texture format parser
---------------------------------------------------------
This (currently a work in progress) crate provides methods for reading and
writing the Bohemia Interactive PAA (PAX) image format used in the ArmA game
series.  The primary source of information on this format is the [Biki],
complemented by the [PMC Editing Wiki].

### Examples
```rust,no_run
fn main() -> anyhow::Result<()> {
    use std::{fs::File, path::Path};

    use a3_paa::*;
    use image::{ImageFormat, RgbaImage};
    use anyhow::Context;

    // Decoding a PAA image
    let mut paa_file = File::open("sky_clear_sky.paa")?;
    let image: PaaImage = PaaImage::read_from(&mut paa_file)?;
    let decoder: PaaDecoder = PaaDecoder::with_paa(image);
    let image: RgbaImage = decoder.decode_first()?;
    image.save_with_format("sky_clear_sky.png", ImageFormat::Png);

    // Reading TexConvert.cfg (needed for encoding settings)
    let tc = std::fs::read_to_string("C:\\Program Files (x86)\\Steam\\steamapps\\\
      common\\Arma 3 Tools\\TexView2\\TexConvert.cfg")
      .context("Could not read TexConvert.cfg")?;
    let hints: TextureHints = TextureHints::try_parse_from_str(&tc)?;

    // Encoding a PAA image
    let image_filename = Path::new("sky_clear_sky.png");
    let image = image::open(image_filename)?.into_rgba8();
    let suffix: String = TextureHints
      ::texture_filename_to_suffix(&image_filename)
      .context("Suffix not found in texture path")?;
    assert_eq!(suffix, "SKY");
    let settings = hints.get(&suffix).context("SKY texture type not found")?;
    let encoder: PaaEncoder = PaaEncoder::with_image_and_settings(image, settings.clone());
    let paa: PaaImage = encoder.encode()?;
    std::fs::write("sky_clear_sky.paa", paa.to_bytes()?)?;

    Ok(())
}
```

### `paatool`
The CLI to `a3-paa`.
```sh
cargo install --force --git=https://github.com/IrregularElements/a3-paa paatool
paatool --help
```

### Roadmap
+ [ ] Annotating PAAs at byte level
+ [ ] Decoding PAAs from:
  + [ ] OFP demo index palette PAX (no TAGGs)
  + [ ] Index palette (0x4747) RGB PAAs
  + [x] ArmA2/3 PAAs
  + [x] ARGB4444
  + [x] ARGB1555
  + [ ] ARGB8888
  + [ ] AI88
  + [x] DXT1
  + [ ] DXT2, DXT3, DXT4
  + [x] DXT5
+ [ ] Encoding images
  + [ ] AVGC, MAXC
  + [x] TexConvert.cfg config language,
  + [ ] TexConvert.cfg encoding rules:
    + [x] Swizzling
    + [x] `autoreduce`
    + [ ] `dynRange`
    + [ ] Mipmap filters
    + [ ] Error metrics
  + [ ] Texture filter language (bitfilt)
  + [ ] Procedural texture generation language (PROCTAGG)
  + [ ] LZSS checksum
+ [x] `paatool`:
  + [x] Encode and decode PAAs
  + [x] Show PAA info
  + [x] Convert DDS to PAA
    + [x] DXTn
    + [ ] ARGB4444
    + [ ] ARGB1555
    + [ ] ARGB8888
    + [ ] AI88

[Biki]: https://community.bistudio.com/wiki/PAA_File_Format
[PMC Editing Wiki]: https://pmc.editing.wiki/doku.php?id=arma:file_formats:paa
