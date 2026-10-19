# Asset provenance

Where each bundled font came from, under which licence, and how any converted file was made.

## Hershey fonts (`fonts/hershey/`)

- **Licence.** The Hershey fonts were created by Dr A. V. Hershey at the U.S. National Bureau of Standards. The Usenet distribution may be used for any purpose provided its acknowledgements travel with the data and the data is not converted to the NTIS format. The full distribution notice is in `fonts/hershey/NOTICE`.
- **Source.** https://github.com/kamalmostafa/hershey-fonts at commit `1356bf2f83d380fcef68c887e88675eb9d445d86`. Only the glyph data and the notice are used; the repository's GPL code is not.
- **Files fetched.**
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/rowmans.jhf
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/rowmand.jhf
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/scriptc.jhf
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/scripts.jhf
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/gothiceng.jhf
  - https://raw.githubusercontent.com/kamalmostafa/hershey-fonts/1356bf2f83d380fcef68c887e88675eb9d445d86/hershey-fonts/hershey.txt, copied verbatim to `fonts/hershey/NOTICE`.
- **Conversion.** Each `.jhf` file maps its first 95 records to ASCII 32 to 126.

  ```sh
  cargo run --example stroke_font -- hershey rowmans.jhf assets/fonts/hershey/hershey-simplex.odsstroke.json --name "Hershey Simplex Roman"
  cargo run --example stroke_font -- hershey rowmand.jhf assets/fonts/hershey/hershey-duplex.odsstroke.json --name "Hershey Duplex Roman"
  cargo run --example stroke_font -- hershey scriptc.jhf assets/fonts/hershey/hershey-script-complex.odsstroke.json --name "Hershey Complex Script"
  cargo run --example stroke_font -- hershey scripts.jhf assets/fonts/hershey/hershey-script-simplex.odsstroke.json --name "Hershey Script Simplex"
  cargo run --example stroke_font -- hershey gothiceng.jhf assets/fonts/hershey/hershey-gothic-english.odsstroke.json --name "Hershey Gothic English"
  ```

## Relief SingleLine (`fonts/relief-singleline/`)

- **Licence.** SIL Open Font License 1.1, in `fonts/relief-singleline/OFL.txt`. No Reserved Font Name is declared.
- **Version.** 2.000. See `fonts/relief-singleline/NOTICE` for how the version was read.
- **Source.** https://github.com/isdat-type/Relief-SingleLine at commit `01dfc5779ec1e9e4b288d96c6c96c23bfccbaf9d`.
- **Files fetched, kept unchanged.**
  - https://raw.githubusercontent.com/isdat-type/Relief-SingleLine/01dfc5779ec1e9e4b288d96c6c96c23bfccbaf9d/fonts/open_svg/ReliefSingleLineSVG-Regular.svg
  - https://raw.githubusercontent.com/isdat-type/Relief-SingleLine/01dfc5779ec1e9e4b288d96c6c96c23bfccbaf9d/OFL.txt
- **Conversion.**

  ```sh
  cargo run --example stroke_font -- svg assets/fonts/relief-singleline/ReliefSingleLineSVG-Regular.svg assets/fonts/relief-singleline/relief-singleline.odsstroke.json --name "Relief SingleLine"
  ```

## Inter and Inter Display (`fonts/Inter-*.ttf`, `fonts/InterDisplay-SemiBold.ttf`)

From the rsms/inter release (https://github.com/rsms/inter), under the SIL Open Font License 1.1 in `fonts/OFL.txt`, as `fonts/LICENCE-NOTE.txt` records. The release version and commit were not recorded when they were added.

## Space Grotesk (`fonts/SpaceGrotesk.ttf`)

From the google/fonts repository, under the SIL Open Font License 1.1 in `fonts/OFL-SpaceGrotesk.txt`, as `fonts/LICENCE-NOTE.txt` records. The source URL and commit were not recorded when it was added.

## Monogram fonts (`fonts/DMSerifDisplay-Regular.ttf`, `fonts/GreatVibes-Regular.ttf`, `fonts/Oswald-Bold.ttf`)

Bundled for the Monogram Maker's themes (spec 42), so Classic, Script, Sports and Botanical look the same on every platform. Each file is kept byte for byte as fetched. Only the weights the themes use are bundled.

- **DM Serif Display** (`fonts/DMSerifDisplay-Regular.ttf`). SIL Open Font License 1.1, in `fonts/OFL-DMSerifDisplay.txt`. Upstream https://github.com/googlefonts/dm-fonts. Fetched from the google/fonts repository at commit `b7c8331cf620346575487ef8db0e2c729f014675`:
  - https://raw.githubusercontent.com/google/fonts/b7c8331cf620346575487ef8db0e2c729f014675/ofl/dmserifdisplay/DMSerifDisplay-Regular.ttf (SHA-256 `8cc3643535edf039aa5d95440a8542735e9197e4f4b8d9303e980fefbf5ab616`)
  - https://raw.githubusercontent.com/google/fonts/b7c8331cf620346575487ef8db0e2c729f014675/ofl/dmserifdisplay/OFL.txt

  The high-contrast serif for Classic and Botanical. Playfair Display was tried first, but upstream publishes it only as a variable font, and `ttf-parser` outlines some of its glyphs wrongly (its "B" loses a counter), so the static DM Serif Display Regular is bundled instead. The italic is not bundled.
- **Great Vibes** (`fonts/GreatVibes-Regular.ttf`). SIL Open Font License 1.1, in `fonts/OFL-GreatVibes.txt`. Upstream https://github.com/googlefonts/great-vibes. Fetched from the google/fonts repository at commit `b7c8331cf620346575487ef8db0e2c729f014675`:
  - https://raw.githubusercontent.com/google/fonts/b7c8331cf620346575487ef8db0e2c729f014675/ofl/greatvibes/GreatVibes-Regular.ttf (SHA-256 `8d509802186f1b51572531ecf313e8098f9a5bfdfaca93f0c9b34467f9982d15`)
  - https://raw.githubusercontent.com/google/fonts/b7c8331cf620346575487ef8db0e2c729f014675/ofl/greatvibes/OFL.txt
- **Oswald Bold** (`fonts/Oswald-Bold.ttf`). SIL Open Font License 1.1, in `fonts/OFL-Oswald.txt`. Fetched from the upstream repository https://github.com/googlefonts/OswaldFont at commit `89795261ac9eeb9aa8cd99f43982c4e4b0e53261`, the commit google/fonts pins. Only the Bold weight is bundled; it is registered as the family "Oswald", so every style of Oswald sets in Bold.
  - https://raw.githubusercontent.com/googlefonts/OswaldFont/89795261ac9eeb9aa8cd99f43982c4e4b0e53261/fonts/ttf/Oswald-Bold.ttf (SHA-256 `eb7d46f856dd57f18a8c03d033c57802692bf127f01dbd95ba8984338e6b5135`)
  - https://raw.githubusercontent.com/googlefonts/OswaldFont/89795261ac9eeb9aa8cd99f43982c4e4b0e53261/OFL.txt

## Monogram frames (`monogram/frames/`)

Original artwork drawn for Kerf for spec 42. Nothing in them is copied or traced from Cricut Design Space or any other product. They are part of Kerf and carry its licence.
