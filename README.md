`a3-paa`: Bohemia Interactive's PAA texture format parser
---------------------------------------------------------

This (currently a work in progress) crate provides methods for reading and
writing the Bohemia Interactive PAA (PAX) image format used in the ArmA game
series.  The primary source of information on this format is the [Biki],
complemented by the [PMC Editing Wiki].

### Roadmap
+ [ ] Annotating PAAs at byte level
+ [ ] Decoding PAAs from:
  + [ ] Index palette RGB
  + [x] ARGB4444
  + [x] ARGB1555
  + [ ] ARGB8888
  + [ ] AI88
  + [ ] DXT1..DXT4
  + [x] DXT5
+ [ ] Encoding images

[Biki]: https://community.bistudio.com/wiki/PAA_File_Format
[PMC Editing Wiki]: https://pmc.editing.wiki/doku.php?id=arma:file_formats:paa
