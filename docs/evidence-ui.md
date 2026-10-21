# Cricut Design Space canvas UI: evidence

> **Note (issue #42).** The icons and shape thumbnails this document says were copied into `assets/ds/` have been removed from the repository. Kerf now uses Lucide icons in `assets/icons`; see `assets/PROVENANCE.md`. Statements below about `assets/ds/icons` and `assets/ds/shapes/previews` describe the removed files.

Read-only investigation of the Design Space desktop app (Angular in Electron), for the egui clone. Sources are the extracted asar under `scratchpad/asar/` and the pretty-printed bundles under `scratchpad/pretty/` (session scratchpad: `/private/tmp/claude-501/-Users-alan-development-rea/805544e1-64f0-4326-b96a-e97af3bdac51/scratchpad/`). `main.js:N` and `chunk-JVMBAEXI.js:N` refer to the pretty-printed files; CSS lines refer to `scratchpad/css/styles.pretty.css` (prettier copy of `design-space-app/styles.css`). Reference screenshots: `images/1.png` (blank canvas) and `images/2.png` (Shapes panel, operation dropdown, Project Preview).

Assets: `assets/ds/fonts/` (see `LICENCE-NOTE.txt`, commercial fonts, local reference only), `assets/ds/icons/` (extracted from the inline SVG registry in `chunk-GJSEQL7X.js`), `assets/ds/shapes/` (reconstructed geometry; Cricut's own path data is encrypted in the offline cache).

Where the code delegates to `CriCanvas.wasm` (mat splitting, tool availability) the rule is inferred and marked as such.

## 1. Design tokens

Citation conventions. `main.js:N` and `chunk-JVMBAEXI.js:N` are lines in `scratchpad/pretty/`. `styles.css:N` is a line in `scratchpad/css/styles.pretty.css` (prettier 3.3.3 output of the one-line `design-space-app/styles.css`). Non-pretty bundles are cited as `file@byte` (offset from `grep -ob`).

There is no design-token layer to copy. The Patch design system is compiled from SCSS to literal hex values in each component's `styles:` array. Only ten `--patch-*` custom properties exist in the whole asar, all for banners and one button override. The tables below are the literal values, grouped by role.

### Colours

| Role | Value | Citation |
|---|---|---|
| Cricut green (primary button bg, active rail text, links, focus outline, checkmarks) | `#00855d` | `chunk-GJSEQL7X.js@1289599` (`.patch-button--type-primary{background-color:#00855d;color:#fff}`); `main.js:256906` (rail active `color:#00855d`) |
| Primary hover / active | `#006648` / `#005239` | `chunk-GJSEQL7X.js@1289599` |
| Alt primary vars (banner-scoped) | `--patch-button-primary-bg:#00855d`, `-hover-bg:#006e4d`, `-active-bg:#005a3f` | `main.js` (grep `--patch-button-primary`) |
| Green tint (rail active bg, selected menu item bg, project-name input selection) | `#ebfaf5` | `main.js:256906`; `chunk-GJSEQL7X.js@1390223` (`.patch-menu-item--selected …{background:#ebfaf5!important;color:#00855d}`); `main.js:254985` |
| Legacy Cricut green (anchors, bounding-box label bg) | `#00a977` | `styles.css:10050`; `main.js:82540` (`BOUNDING_BOX_LABEL_BACKGROUND_COLOR`) |
| Link button primary | `var(--patch-link-button-primary, #00855d)` | `chunk-GJSEQL7X.js` (`.patch-link-button--type-primary`) |
| Beta pill bg / hover / text | `#0c80e4` / `#0063b8` / `#fff`, Graphik 500 12px/16px, `padding:4px 8px`, `border-radius:4px` | `design-space-tabs/main.js@1815950` (`.badge`) |
| Primary text | `#121212` | `styles.css:16033` (body); toolbar trigger `main.js:167193` |
| Rail label text | `#3a404a` | `main.js:256906` (`.cr-navbar-icon a{color:#3a404a}`) |
| Rail hover bg | `#e6e6e6` | `main.js:256906` (`.newNav .nav .cr-navbar-icon a:hover{background-color:#e6e6e6}`) |
| Rail right border | `1px solid #e6e6e6` | `main.js:256906` |
| Rail section divider | `hr` 1px `#ccc`, container `height:16px;padding:6px 8px` | `main.js:256906` |
| Toolbar bottom border | `1px solid #cccccc` | `main.js:167193` (`.canvas-toolbar-v2`) |
| Toolbar item divider | 1px x 16px `#adadad` | `main.js:167193` (`.canvas-toolbar-v2__divider:before`) |
| Toolbar trigger hover / active bg | `#e6e6e6` | `main.js:167193` |
| Toolbar disabled item | `opacity:.4; pointer-events:none` | `main.js:167193` (`.canvas-toolbar-v2__item--disabled`) |
| Header undo/redo divider | 1px x 24px `#ccc`, radius 8px, margin `0 8px` | `main.js:265993` |
| Split gutter (panel edges) | 1px `#e6e6e6`; dragged `#00855d` | `main.js:263167` (`as-split … .as-split-gutter-icon`) |
| Menu item hover bg | `#f5f5f5` | `chunk-GJSEQL7X.js@1390223` |
| Menu item prefix icon | `#595959`; suffix `#737373`; disabled `#adadad` | `chunk-GJSEQL7X.js@1390223` |
| Disabled button text / bg | `#adadad` / `#f5f5f5`; secondary disabled ring `inset 0 0 0 1px #ccc` | `chunk-GJSEQL7X.js@1289599` |
| Title bar bg (focused / blurred) | `#f5f5f5` / `#8f8f8f` | `design-space-tabs/main.js@1815950` (`--title-bar-background-color`) |
| Active tab bg (focused / blurred) | `#ffffff` / `#adadad`; tab text `#404040` / `#adadad` | `design-space-tabs/main.js@1815950` |
| Home tab text | `#00855d` | `design-space-tabs/main.js@1767148` |
| Error / badge red | `#ba3003` (button), `#db3903` (warnings badge) | `chunk-GJSEQL7X.js@1289599`; `main.js:265993` |
| Ruler bg | `#f5f5f5` | `main.js:82528` |
| Ruler numbers | `#595959` | `main.js:82529` |
| Ruler ticks and border | `#b8b8b8` (`MAJOR_GRID_DEFAULT_DARK_COLOR`) | `chunk-JVMBAEXI.js:202984` |
| Grid major / minor (light bg) | `#b8b8b8` / `#dbdbdb` | `main.js:82525-82527`, defaults at `chunk-JVMBAEXI.js:202977-202978`, light/dark switch at `:203786-203794` |
| Grid major / minor (dark bg) | `#ebebeb` / `#919191` | `main.js:82524-82526` |
| Canvas bg | `#FFFFFF` | `main.js:82519` |
| Selection box (new) | `#31C498`, stroke 2 | `main.js:82533`, `:82542` |
| Selection crosshair | `#919191`, width 1, size 25 | `main.js:82534-82536` |
| Smart guide / layer guide | `#D6008B` | `main.js:82582`, `:82601` |
| Text highlight / placeholder text stroke | `#0070D1` | `main.js:82522` |
| Artboard stroke | `#ADADAD` | `main.js:82661` |
| Scrollbar handle / track | `#C2C2C2` / `#FAFAFA` | `main.js:82530-82531` |

### Typography

Font faces (`styles.css:15451-15515`):

| `font-family` | weight | file |
|---|---|---|
| `Euclid` | 400 | `media/Euclid-Circular-B-Regular.otf` |
| `Euclid` | 500 | `media/Euclid-Circular-B-Medium.otf` |
| `Euclid` | 600 | `media/Euclid-Circular-B-Semibold.otf` |
| `Euclid` | 700 | `media/Euclid-Circular-B-Medium.otf` (700 maps to Medium) |
| `Graphik` | 400 / 400 italic | `media/Graphik-Regular.woff2` / `Graphik-RegularItalic.woff2` |
| `Graphik` | 500 | `media/Graphik-Medium.woff2` |
| `Graphik` | 600 and 700 | `media/Graphik-Semibold.woff2` |
| `Cricut Gascogne` | 400, 700 | `media/Cricut_Gascogne_Book.woff` |

Lato 300 (`styles.css:12027`) and Roboto 300/400/500 (`styles.css:12050` onwards) are declared for legacy Bootstrap/Material pieces. They do not appear on the canvas screen. Glyphicons (`styles.css:3037`) and FontAwesome 4.7.0 (`styles.css:6724`) are also declared.

The app body is `Graphik, Arial, sans-serif` 400 16px/24px `#121212` with `-webkit-font-smoothing: subpixel-antialiased` (`styles.css:16028-16035`). The component styles in `main.js` use `font-family:Graphik,Arial,sans-serif` 274 times and `Euclid,Arial,sans-serif` 27 times. Euclid is used only for the `patch-display-*` headings.

Patch type scale (`styles.css:15517-15643`):

| class | family | weight | size / line-height |
|---|---|---|---|
| `patch-display-xs` | Euclid | 500 | 20 / 24 |
| `patch-display-s` | Euclid | 500 | 24 / 28 |
| `patch-display-m` | Euclid | 500 | 30 / 36 |
| `patch-display-l` | Euclid | 500 | 36 / 42 |
| `patch-display-xl` | Euclid | 500 | 44 / 48 |
| `patch-body-xs-400/500/600` | Graphik | 400/500/600 | 12 / 16 |
| `patch-body-s-400/500/600` | Graphik | 400/500/600 | 14 / 20 |
| `patch-body-m-400/500/600` | Graphik | 400/500/600 | 16 / 24 |
| `patch-body-l` | Graphik | 400 | 20 / 28 |
| `patch-body-xl` | Graphik | 400 | 22 / 28 |
| `patch-caption-m-400/500/600` | Graphik | 400/500/600 | 10 / 16 |

Sizes on the canvas screen:

- Rail labels are 10px. `.cr-navbar-icon{font-size:10px}`, the label line-height is 16px, and two-line labels use 15px (`main.js:256906`).
- Toolbar labels are Graphik 400 14px. The toolbar host is 10px/16px and `.canvas-toolbar-v2__trigger span{font-size:14px;font-weight:400}` (`main.js:167193`).
- Tab labels are Graphik 500 14px/20px (`design-space-tabs/main.js@1767148`).
- Title-bar buttons (Help, machine and so on) are Graphik 400 14px/20px, `height:32px`, `border-radius:4px` (`design-space-tabs/main.js@1815950`).
- The project name is 13px. `.project-header__name{font-size:13px;max-width:300px}` (`main.js:254985`).
- Patch buttons are Graphik 500. Medium is 14px, `height:40px`, `padding:8px 24px`. Small is 12px, `height:32px`, `padding:4px 16px`. Large is 16px, `height:56px` (`chunk-GJSEQL7X.js@1289599`).
- Menu items are Graphik 400 14px, `height:32px`, `padding:0 8px`, `margin:2px 0` (`chunk-GJSEQL7X.js@1390223`).
- The zoom control is Graphik 400 14px/20px `#121212` (`main.js:179797`).
- Ruler numbers are `12 * devicePixelRatio px Arial` (`chunk-JVMBAEXI.js:200867`). This is the CriCanvas renderer. The legacy renderer used `FONT_STACK "'Graphik', Arial, sans-serif"` at `FONT_SIZE 12` (`main.js:82563-82564`).

### Radii and shadows

| Item | Value | Citation |
|---|---|---|
| Patch button | `border-radius:9999px`, `min-width:132px`, `max-width:320px`, `gap:4px` (8px for medium) | `chunk-GJSEQL7X.js@1289599` |
| Secondary button (Preview) | white bg, `#00855d` text, `box-shadow: inset 0 0 0 1px #00855d`; hover fills `#00855d` with white text | same |
| Rail item | `border-radius:8px` | `main.js:256906` |
| Toolbar trigger | `border-radius:8px`, `padding:4px 8px`; icons 20x20 | `main.js:167193` |
| Popover | `border-radius:4px`, `border:1px solid rgba(18,18,18,.15)`, `box-shadow:0 3px 8px #12121233` | `chunk-GJSEQL7X.js@1429039` |
| Menu (operation dropdown and others) | `min-width:200px`, `border-radius:4px`, `padding:16px 0`, `box-shadow:0 10px 20px #12121233` | `chunk-GJSEQL7X.js@1390223` |
| Header bar | `box-shadow:0 1px 3px #12121233` | `styles.css:11086` |
| Zoom pill | `border-radius:40px`, `border:1px solid #cccccc`, white | `main.js:179797` |
| Content-panel collapse tab | `border-radius:0 8px 8px 0`, `box-shadow:3px 0 5px #12121226`, 12x90px | `main.js:263167` |
| Active tab | `border-radius:4px 4px 0 0`, with 10px inverse-radius corners drawn by `box-shadow` pseudo-elements | `design-space-tabs/main.js@1767148` |
| Popover with banner | `border-radius:9px` (repeated at the top of every component style) | everywhere |

### Layout metrics

| Metric | Value | Citation |
|---|---|---|
| Title/tab bar height | `44px`; macOS `padding-left:88px` for traffic lights; `padding-right:22px` | `design-space-tabs/main.js@1815950` |
| Tab strip | `height:40px; margin-top:4px`; tab `padding:0 24px`; Home tab `min-width:104px; padding:0 15px` | `design-space-tabs/main.js@1767148` |
| Project header bar | `height:64px`, `padding:12px 24px`, white | `styles.css:11077-11094` (`app-header-bar .header-bar`), used by the canvas at `main.js:265830` |
| Header right-side button gap | `10px` | `main.js:265993` (`.header-bar__content__right__buttons`) |
| Left rail width | `70px` (min and max) | `main.js:256906` (`.cr-sidebar-left-nav{max-width:70px;min-width:70px;top:64px}`) |
| Rail item | `54px` wide x `60px` tall, `margin:8px`, radius 8 | `main.js:256906` (`.newNav .nav .cr-navbar-icon`) |
| View container offset | `left:70px; top:64px` | `main.js:265993` (`.cr-home-design .cr-view-container`) |
| Edit toolbar (v2) height | `40px` (`min-height:40px;max-height:50px;padding:4px`); `90px` when the text toolbar shows | `main.js:167193`; `main.js:263167` (`.cr-home-canvas-subtoolbar--v2{height:40px}`) |
| Legacy toolbar height | `50px` | `main.js:263167` |
| Content (Shapes etc.) panel width | `336px`, locked, gutter `4px` | `main.js:262836-262839` (`as-split unit="pixel" gutterSize="4"`, `as-split-area size="336"`) |
| Right panel (Project Preview / Layers) width | `312px` | `main.js:262891`, `:262997` |
| Visualisation sidebar var | `--visualization-sidebar-width: 280px` | `main.js` (grep) | Consistent with the 312px split area: 280px content plus 16px margin each side.
| Ruler thickness | `24px` (CriCanvas, `DEFAULT_CRICANVAS_RULER_HEIGHT`); legacy renderer `23px` | `main.js:82574`, `:82573` |
| Zoom pill | `min-width:155px; height:36px` (`130px` when zoom-to-fit is off) | `main.js:179797` |
| Zoom pill position | `.zoom-ui{position:absolute;bottom:25px;left:45px}` | `main.js:263167` |

The reference screenshot `images/2.png` is 2000px wide at about 1.143 screenshot px per CSS px. Rail 80px / 1.143 = 70. Header 75 / 1.143 = 66, against 64. Toolbar 45 / 1.143 = 39, against 40. Tab bar 52 / 1.143 = 45.5, against 44. Make button 150 / 1.143 = 131, against `min-width:132`. Shapes panel 390 / 1.143 = 341, against 336 + 4. All agree with the code values.

### Fonts copied

`/Users/alan/development/rea/cricut-lite/assets/ds/fonts/` holds `Euclid-Circular-B-{Regular,Medium,Semibold}.otf` and `Graphik-{Regular,RegularItalic,Medium,Semibold}.woff2`, copied byte for byte from `design-space-app/media/`.

No licence file ships with either font. The Euclid name table reads: "Copyright (c) 2018 Swiss Typefaces Sarl … Please refer to the license you accepted when purchasing. http://www.swisstypefaces.com/licensing/#retail-font-software-licence". Graphik is a Commercial Type face. Its woff2 is compressed, so I did not read its name table. Neither font is mentioned in `static-asset/design3/license.txt` or any `3rdpartylicenses.txt`.

See `assets/ds/fonts/LICENCE-NOTE.txt`. These fonts are for local reference only. A public build needs an open substitute.

### Not found

- There are no CSS custom properties for the core palette, so there are no token names to mirror.
- The "Start Free Trial" pill colours are not in this section. That is a header item, covered in section 8.

## 2. Left rail and icons

Paths are relative to the scratchpad. `pretty/main.js` is the prettified `asar/design-space-app/main.js`. `prettytabs/tabs-main.js` is `asar/design-space-tabs/main.js`, prettified with prettier 3.3.3 for this report.

### Icon system

The icons are not an icon font and are not per-file assets. Every `patch-icon` resolves a name through an inline SVG registry compiled into `asar/design-space-app/chunk-GJSEQL7X.js`. Each entry has the shape `{name:"<icon>-size-<N>",data:'<svg ...>'}` (for example at byte 734427: `var Wa={name:"rocketship-size-24",data:'<svg width="24" height="24" viewBox="0 0 24 24" ...'}`). The registry holds 769 entries covering 366 icon names. Most names come in sizes 24 and 32, and some also come in 16. Paths use `fill="currentColor"`.

The `patch-icon` component is in `chunk-GJSEQL7X.js` at byte offset 1255453 (`this.DEFAULT_SIZE=24`, followed by `selectors:[["patch-icon"]]`). It sets `DEFAULT_SIZE=24`, `DEFAULT_SIZE_LARGE=32` and `DEFAULT_COLOR="inherit"`. `_getMatchingIcon(name,size)` looks up `${name}-size-${size}`. If that is missing, it falls back to `-size-24` when size <= 24 and to `-size-32` otherwise. Size is valid when it is a multiple of 4 up to 24, or a multiple of 8 above 24. The component writes `width`/`height` = `${size}px` onto the SVG and adds the classes `patch-icon` and `patch-icon--inherit`. Its host style is `.patch-icon{color:#121212}`, `.patch-icon--inherit{color:inherit}`, `display:inline-block;line-height:0`.

The extractor script is `scripts/extract_icons.py`, and it writes the full dump to `iconreg/`. The FontAwesome and Glyphicons fonts in `media/` are legacy and are not used by the rail, header or toolbar.

### Left rail component

- Selector: `canvas-routing-sidebar-left`, `pretty/main.js:256587`. The template is at `:256870`, the row sub-templates are in `R_t` at `:252629`, and the conditional New/Templates/Projects/Monogram/Background/Layouts/Create AI rows are at `:252380-252628`.
- Root markup: `div.cr-sidebar-left.cr-sidebar-left-nav.newNav > nav > ul.nav > li.cr-navbar-icon.hover-link2 > a.cr-navbar-icon__container > patch-icon + span.cr-leftnav-label.hkey-first-letter`. Divider rows are `li.divider > hr`.
- Icons are rendered without a `size` input, so they use the 24 px default (consts at `:256600-256780`).

Item order, from `R_t` at `pretty/main.js:252629-252760`. The const indices refer to the consts array at `:256595`.

| # | Label (i18n key, en value) | patch-icon name | data-test-id | Condition |
|---|---|---|---|---|
| 1 | New (`CANVAS_MENU_NEW`, global-en_US.txt:586) | `plus-circle` | SidebarLeft-NewCanvas-Anchor / -NewCanvasWithMenu-Anchor | hidden for Gemini/Scout machines (`isGeminiOrScoutMachine()`) |
| 2 | Layouts (old position, `CANVAS_MENU_LAYOUTS`) | `template` | SidebarLeft-Template-Anchor | only when `DH_NEW_TEMPLATES_PANEL` is false and not artboard |
| 3 | Templates (`CANVAS_MENU_TEMPLATES` = 'Templates', :613) | `image-edit` | SidebarLeft-TemplateSearch-Anchor | `DH_NEW_TEMPLATES_PANEL` and artboard and not PTC project |
| 4 | Projects (`CANVAS_MENU_PROJECTS`, :606) | `rocketship` | SidebarLeft-ProjectSearch-Anchor | shown unless the new templates panel replaces it |
| - | divider (`li.divider > hr`) | | | |
| 5 | Shapes (`CANVAS_MENU_SHAPES`, :610) | `shapes` | SidebarLeft-BasicShapes-Anchor | always; active when `activeMenuPopup==="shapes"` |
| 6 | Images (`CANVAS_MENU_IMAGES`, :579) | `images` | SidebarLeft-ImageSearch-Anchor | always |
| 7 | Text (`CANVAS_MENU_TEXT`, :614) | `text` | SidebarLeft-Text-Anchor | always; `animation-text` class while `addTextAnimation` is set |
| 8 | Monogram (`CANVAS_MENU_APP_MONOGRAM_MAKER` = 'Monogram', :507) | `monogram-maker` | SidebarLeft-MonogramMaker-Anchor | `showMonogramMakerMenu` and not artboard |
| 9 | Background (`CANVAS_MENU_BACKGROUND`) | `vector-fill` | SidebarLeft-Background-Anchor | `showBackgroundFill()` |
| - | divider | | | |
| 10 | Layouts (`CANVAS_MENU_LAYOUTS` = 'Layouts', :581) | `template` | SidebarLeft-Template-Anchor | `DH_NEW_TEMPLATES_PANEL` true and not artboard; disabled when offline and `cpTemplatesEnabled` is false |
| 11 | Upload (`CANVAS_MENU_UPLOAD`, :616) | `upload` | SidebarLeft-Upload-Anchor | always; disabled offline. Its li also carries `patch-caption-m-400` |
| 12 | Lab (literal "Lab") | `rocketship` | | `showLabs` |
| 13 | Create AI (`CREATE_AI` = 'Create AI', :1629) | `star-cluster` | SidebarLeft-Synthesis-Anchor | Create AI row (`D_t`, :252593) |

The Edit Image route uses a different row set (`v_t`/`j_t`, :252274/:252326). That set includes Details (`COMMON_DETAILS`, icon `small-business`).

The screenshot (images/2.png) matches this order exactly: New, Projects, divider, Shapes, Images, Text, Monogram, divider, Layouts, Upload, Create AI.

Rail styles, inline in the component at `pretty/main.js:256906`:
- `.cr-sidebar-left-nav{max-width:70px;min-width:70px;top:64px}`. The rail is **70 px** wide and starts below the 64 px header.
- `.newNav{border-right:1px solid #e6e6e6}`, `.newNav nav{background:#fff}`.
- Item: `.newNav .nav .cr-navbar-icon{border-radius:8px;height:60px;width:54px;margin:8px}`, `font-size:10px` (from `.cr-navbar-icon`), `a{padding:0;line-height:16px;color:#3a404a}`.
- Hover: `a:hover{background-color:#e6e6e6}`.
- Active: `.active a{background:#ebfaf5;color:#00855d}`.
- Divider: `.divider{height:16px;padding:6px 8px}`, `hr{margin:0;border:none;height:1px;background:#ccc}`.
- Container: `.cr-navbar-icon__container{display:flex;flex-direction:column;width:100%;min-height:70px;justify-content:center}`.
- Label typography: `.patch-caption-m-400{font-family:Graphik,Arial,sans-serif;font-weight:400;font-size:10px;line-height:16px}` (styles.css byte 285342).

### Icons copied to `assets/ds/icons/`

Files are named `<registry-name>-<size>.svg`. They are copied verbatim from the registry and keep `fill="currentColor"`. The script is `scripts/copy_icons.sh`, with the mapping in `icon_map.txt`. All 24 px files were rendered to a contact sheet with qlmanage and checked against the screenshot.

| Role | Registry name | Sizes copied | Where used (citation) |
|---|---|---|---|
| Rail New | plus-circle | 24, 32 | main.js:256741 |
| Rail Projects | rocketship | 24, 32 | main.js:256662 |
| Rail Shapes | shapes | 24, 32 | main.js:256617 |
| Rail Images | images | 24, 32 | main.js:256672 |
| Rail Text | text | 24, 32 | main.js:256636 |
| Rail Monogram | monogram-maker | 24, 32 | main.js:256813 |
| Rail Layouts | template | 24, 32 | main.js:256765 (const 39) |
| Rail Upload | upload | 24, 32 | main.js:256698 |
| Rail Create AI | star-cluster | 24, 32 | main.js:256868 |
| Rail Templates (flagged) | image-edit | 24, 32 | main.js:256781 |
| Rail Background (flagged) | vector-fill | 24, 32 | main.js:256831 |
| Rail Details (edit image) | small-business | 24, 32 | main.js:256652 |
| Tab Home | house-filled | 24, 32 | tabs-main.js:40110 (rendered at size 24, `$G` :39878) |
| Tab new / close | plus, x | 16, 24, 32 | tabs-main.js:40099, :40124 |
| Title bar Help | help | 24, 32 | `tabs-help-menu`, tabs-main.js:40721 |
| Title bar machine | machine | 24, 32 | `app-machine-connection-status-icon`, tabs-main.js:40832 |
| Title bar chat help | help-assistant | 24, 32 | `app-help-menu-chat`, tabs-main.js:41022 |
| Title bar bell | notification | 24, 32 | `app-notifications`, tabs-main.js:40182 |
| Title bar avatar | avatar (+ `avatar-circle.svg` from media/) | 24, 32 | `dir-common-avatar`, tabs-main.js:36774 |
| Title bar profile chevron | chevron-down | 16, 24, 32 | `app-user-profile-menu`, tabs-main.js:40255 |
| Header undo / redo | undo, redo | 24, 32 | `toolbar-undo-redo`, main.js:157868 |
| Header title pencil | edit | 24, 32 | project header const, main.js:254942 (size 16, set in `l_t` :252159) |
| Header rename confirm / cancel | checkmark, x | 16, 24, 32 | main.js:254913, :254923 (size 20, `s_t` :252091) |
| Header title chevron | chevron-down | 16, 24, 32 | main.js:254934 (size 12, `c_t` :252146) |
| Header cloud save | download-cloud | 24, 32 | `canvas-routing-auto-save-menu`, main.js:263648 |
| Header My Stuff | inbox | 24, 32 | main.js:265555 |
| Header Start Free Trial | access-a | 16, 24, 32 | braze default `iconLeft:"access-a"`, design-space-app/chunk-ACGDDC32.js byte 108938 |
| Header marketing ad arrow | arrow-small-right | 24, 32 | braze `marketing-button` default `iconRight` |
| Header offline / warnings / DTF cart | offline, warning, cart | 24/32 (warning also 16) | main.js:265520, :265621, :265649 |
| Operation: Cut | cut (scissors) | 16, 24, 32 | `toolbar-operation-type`, main.js:161064-161073 |
| Operation: Draw | draw (pen) | 16, 24, 32 | same |
| Operation: Print Then Cut | print (printer) | 24, 32 | main.js:161426-161428, :161445 |
| Operation: Guide | guides (hash) | 16, 24, 32 | main.js:161073 |
| Toolbar Format | advanced | 24, 32 | `canvas-ui-toolbar-v2`, main.js:166742 |
| Toolbar Size | enlarge | 24, 32 | main.js:166758 |
| Toolbar Flip | flip-horizontal (+ flip-vertical) | 24, 32 | main.js:166774 |
| Toolbar Arrange | drag-multi | 24, 32 | main.js:166790 |
| Toolbar Align | align-left | 24, 32 | main.js:166806 |
| Toolbar Rotate | angle | 24, 32 | main.js:166822 |
| Toolbar Effects | wand-2 | 24, 32 | main.js:166838 |
| Toolbar submenu arrow / Foil arrow | arrow-right (8x8 triangle; registry has no size suffix) | copied as `arrow-right-8.svg` | main.js:166839 |
| Toolbar Combine | shape-combine | 24, 32 | main.js:166855 |
| Toolbar Contour | contour | 24, 32 | main.js:166871 |
| Toolbar More | chevron-down | 16, 24, 32 | main.js:166888 |
| Layers panel toggle | layers | 24, 32 | `canvas-routing-home-design-canvas` consts, main.js:262936 |
| Panel close / collapse | x, chevron-left | 16, 24, 32 | same block |
| Zoom − / + / fit | minus, plus, zoom-to-fit | 16/24/32 | `canvas-ui-zoom`, main.js:179713 |
| Info | info | 16, 24, 32 | Mat Details (i) etc. |

The toolbar-v2 order is taken from data-test-ids in `canvas-ui-toolbar-v2` (main.js:166551 onward): LineTypeFillType (operation), Format, Size, Flip, Arrange, Align, Rotate, Effects, Combine, Contour, More.

### Not found / caveats

- **"Design Help" with the circular-arrow-and-sparkle icon** (screenshot title bar, and the floating button at the bottom right of the canvas). Neither the label "Design Help" nor that glyph exists in any i18n dump, in `design-space-tabs`, or in the icon registry. The bundled chat entry point is `app-help-menu-chat` (icon `help-assistant`, icon-only). Its experiment variant `app-help-menu-chat-experiment` (tabs-main.js:41170) draws inline 36 px SVGs selected by `iconVariant()`: QUESTION_MARK, TWO_LINES, THREE_STARS, ONE_STAR, or default. These SVGs sit in the consts at tabs-main.js:41190-41290 and were not extracted as files. The labelled "Design Help" button is probably remote config or server-rendered.
- **Start Free Trial ↗ arrow.** The braze `access-button` default has `iconRight:"none"`, so the arrow in the screenshot comes from a remote Braze card's `extras`. The closest registry glyph is `window-popout` or `arrow-small-go`. This is unconfirmed.
- The "lock" icon named in the brief does not appear on Start Free Trial. The button uses `access-a` (the Cricut Access "a"). `lock` exists in the registry if you need it.

## 3. Shapes panel

### Where the list comes from

The shape list is not compiled into the app. It is two Cricut image sets fetched from the images API (or from the offline cache), configured in `AppConfig.FEATURE.BASIC_SHAPES_GROUP_IDS` at `pretty/main.js:83956`:

| Key | Value |
|---|---|
| `FREE_SHAPES` | image set `6679025` |
| `DELUXE_SHAPES` (subscription) | image set `6649312` |
| `LINE_ID` | hexId `M2AAB3D7A` (the Line shape, always sorted first) |
| `SHAPE_GROUPS` | Round 6860227 (order 0), Rectangular 6860238 (1), Triangular 6860253 (2), Multi-faceted 6860271 (3), Plus Equal 6860285 (4), Complex Round 6860299 (5), Banners 6860310 (6), Labels and Tags 6860329 (7), Wide 6860415 (8), Mixed 6860372 (9), Special Symbols 6860387 (10), Quilt 6860396 (11), Envelope/Flap/Slit 6860407 (12) |

`ShapesService` (`var vie`, `pretty/main.js:322575-322690`) loads both sets with `srvcImages.GetImages({imageSetId})` when online, or from `LocalData/<user>/ExtendedShapeIds/{FreeShapeIds,DeluxeShapeIds}` offline. It then sorts with `sortShapeGroupIds` (`main.js:322643`). The Line group goes first, then `SHAPE_GROUPS` by `order`, then "Unclassified". The sort is stable, so API order is kept inside a group. Each shape is placed in the first of its `imageSets` that matches a group id.

The panel component is `app-static-shapes-panel` (`pretty/main.js:124801-124980`). It renders `CANVAS_SHAPES_FREE` ("Free"), a grid of `app-search-lite-tile` (`isShape=true`, `canBeBookmarked=false`), then, if online and `showDeluxeShapes`, the access ribbon, the title `CANVAS_SHAPES_UNSUBSCRIBED_ACCESS_TITLE` ("Included with Subscription"), the subtitle `CANVAS_SHAPES_UNSUBSCRIBED_ACCESS_SUBTITLE` ("Enjoy unlimited use of these with {{linkToCricutAccess}}.", link text `COMMON_CRICUT_ACCESS_TM` "Cricut Access™") and the deluxe grid. i18n keys are at `static-asset/design3/translation/en_US.json:720-725,1246`.

Panel CSS (`main.js:124977`):
- `.cr-shapes-grid-container`: `display:grid; grid-template-columns:repeat(3,1fr); gap:8px; margin-bottom:16px`.
- Ghost tile: 91x91px.
- `.cr-shapes-panel-container`: `padding:0 16px 0 15px`.
- Title: 14px, `#2b2b2b`, white background, sticky at `top:65px`, `padding:8px 0 10px`.
- Subtitle: Graphik 400 14px/20px, `#404040`, `margin-top:20px`.
- Search link colour: `#00855d`.

Tile CSS is in `content-tile` (`pretty/chunk-JAGVFCCQ.js:34926`):
- `border-radius:8px` with background `linear-gradient(180deg,transparent 0%,rgba(255,255,255,.595) .01%,#f5f5f5 52.5%)`.
- Hover: `box-shadow:0 1px 3px #12121233`.
- Selected: `outline:1px solid #00a977`.
- Image inset: `top:12%; left:15%; width:70%; height:70%; object-fit:contain`.

### Shape data and path geometry

The live data was read from the local offline cache at `~/.cricut-design-space/LocalData/14787976/Images/<id>/{Data,Details,Preview.png}`. `Data` holds the `imageModel` with layer metadata. The path itself sits in `imageLayerData[<groupGUID>]` as an **encrypted** blob. `chunk-JVMBAEXI.js:265179` sets `pathEncrypted: !0`. Decryption happens in the native path-operation plugin over IPC: `GetLayerPath` sends `RequestPathOperationParsePath` (`pretty/main.js:283230-283250`). The raw Cricut path data is therefore **not recoverable** from the bundle, and I did not try to break that encryption.

The SVGs in `assets/ds/shapes/` are **clean-room geometric reconstructions**. Their sources:
- The exact native sizes from `Data`.
- Measurements of the 390x390 `Preview.png` silhouettes, made with `scratchpad/tools/measure.py`.
- `scratchpad/tools/genshapes.py`, which generates every SVG and is rerunnable.

Each SVG uses the native unit system of 72 units per inch (`CANVAS_POINTS_PER_INCH = 72`, `main.js:82472`). The original thumbnails are copied to `assets/ds/shapes/previews/<free|sub>-<name>-<id>.png` (all 20 free and 44 subscription shapes).

Free shapes, in display order (matches screenshot 2). "Size" is the native size in 72 dpi units. "Default H" is `imageSourceHeightDefault` in inches.

| # | Name (API) | Image id | Size (W x H) | Default H (in) | Fill / type | SVG | How built |
|---|---|---|---|---|---|---|---|
| 1 | Line | 715865466 | 0 x 180 | 2.5 | `#4A494A`, output `SCORE`, 1 open contour | `01-line.svg` | exact: `M0,0 L0,180` |
| 2 | Triangle (down) | 715865518 | 119.2 x 180 | 2.5 | #000000 CUT | `02-triangle-down.svg` | exact polygon |
| 3 | Triangle (equilateral) | 715865417 | 144 x 124.7 | 1.7319 | #000000 CUT | `03-triangle.svg` | exact polygon |
| 4 | Diamond | 715865440 | 103.9 x 180 | 2.5 | #000000 CUT | `04-diamond.svg` | exact rhombus |
| 5 | Star | 715865455 | 144 x 137 | 1.9028 | #000000 CUT | `05-star.svg` | regular 5-point, inner/outer radius 0.506 (fitted) |
| 6 | Star (thin, API name "Star ") | 715865558 | 144 x 137 | 1.9028 | #000000 CUT | `06-star-thin.svg` | regular 5-point, inner/outer 0.39 (fitted, close to the 0.382 pentagram) |
| 7 | Octagon | 715865406 | 144 x 144 | 2 | #000000 CUT | `07-octagon.svg` | regular, flat sides |
| 8 | Pentagon | 715865505 | 144 x 137 | 1.9028 | #000000 CUT | `08-pentagon.svg` | regular, point up |
| 9 | Hexagon | 715865614 | 144 x 124.7 | 1.7319 | #000000 CUT | `09-hexagon.svg` | regular, points left/right, flat top |
| 10 | Heart | 715865430 | 144.15 x 127.375 | 1.7681 | #000000 CUT | `10-heart.svg` | approximate cubic Béziers (notch depth about 12.5% of H, lobe tops at x about 27% and 73%) |
| 11 | Plus Sign | 715865449 | 144 x 144 | 2 | #000000 CUT | `11-plus.svg` | arm width 25.2 (0.175 W, measured) |
| 12 | Equal Sign | 715865551 | 144 x 77.2 | 1.0722 | #000000 CUT, 2 contours | `12-equals.svg` | bars 25 high, gap 27.2 (measured) |
| 13 | Circle | 715865853 | 144 x 144 | 2 | #000000 CUT | `13-circle.svg` | exact |
| 14 | Rectangle | 715865602 | 179.9 x 119.9 | 1.6667 | #000000 CUT | `14-rectangle.svg` | exact |
| 15 | Square | 715865376 | 144 x 144 | 2 | #000000 CUT | `15-square.svg` | exact |
| 16 | Oval (pill) | 715865495 | 180 x 120 | 1.6667 | #000000 CUT | `16-pill.svg` | stadium, end radius 60 |
| 17 | Square (rounded) | 715865480 | 143.9 x 143.9014 | 2.00009 | #000000 CUT | `17-rounded-square.svg` | corner radius about 13 (0.09 W, measured) |
| 18 | Oval (ellipse) | 715865391 | 180 x 127.2 | 1.7667 | #000000 CUT | `18-oval.svg` | exact ellipse |
| 19 | Pennant (arch / U) | 715865605 | 119.2 x 180 | 2.5 | #000000 CUT | `19-arch-pennant.svg` | rectangle with semicircular bottom, radius 59.6 |
| 20 | Half Circle | 715865474 | 90 x 180 | 2.5 | #000000 CUT | `20-half-circle.svg` | flat edge on the right, arc on the left |

The ordering mechanism works like this. Rows 1-12 sort by group: Line, then Triangular (2-4), Multi-faceted (5-10), Plus Equal (11-12). Rows 13-20 are "Unclassified" and keep the `FreeShapeIds` order: `[715865853, 715865455, 715865558, 715865449, 715865518, 715865602, 715865376, 715865495, 715865406, 715865466, 715865417, 715865505, 715865614, 715865480, 715865391, 715865430, 715865605, 715865474, 715865440, 715865551]`.

Subscription ("Deluxe") shapes: 44 in total, each with `entitlementLabel` "£0.89" (GBP locale). The first three on screen (Complex Round group, order 5) are:

| Name | Image id | Size (W x H) | Default H (in) | SVG | How built |
|---|---|---|---|---|---|
| Tear Drop | 715865795 | 134.0009 x 180.0013 | 2.50009 | `sub-01-teardrop.svg` | point at top, bottom circle radius 67 centred at y=113, convex sides (approximate) |
| Sun Silhouette (the "starburst") | 715865687 | 144 x 144 | 2 | `sub-02-starburst-sun-silhouette.svg` | 32 points; valleys at 0.905 R joined by inward quadratic scallops (measured from radial profile) |
| Crescent (API name "Crescent ") | 715865666 | 125.272 x 179.9735 | 2.50003 | `sub-03-crescent.svg` | outer circle r=90 at (90,90) minus inner circle r=87.8 at (158.4,90), x scaled 0.9859 to the native width |

The full deluxe list, with names, ids, native sizes and default heights, is dumped by `scratchpad/tools/listshapes.py` to `scratchpad/ui-sections/shapes-cache-dump.txt`. The previews of all 44 are in `assets/ds/shapes/previews/sub-*.png`. Names include Ribbon, Elongated Pentagon, Trapezoid, Overlapping Ovals, Heptagram, Check Mark, Open Ended Circle/Square/Triangle/Hexagon, Swallowtail x2, Angle Bracket, Gift Tag Shape, Rectangle With Cut Off Corners, Paralellogram, Diamond, Stretched Oval, Segmental Arch, Square With Inverted Corners, Rectangle With Rounded Corner, Single Scale, Crescent (second, 715865768), Rounded Gift Tag, Quatrefoil, Heart, Multiply, Arrow, Banner Pennant, Scalloped Circle, Pointed Scale Shape, Scalloped Border, Tilde, Squircle, Decorative Label, Ribbon Banner, Open Ended Square with Rounded Corners, Elongated Scallop, Cake Topper Stick, Swallowtail Banner and Arch.

### Default size and colour on insert

- **Size.** `InsertImagesCanvasPOC` (`pretty/main.js:180248-180330`) scales the group uniformly so that its height equals `imageSourceHeightDefault * CANVAS_POINTS_PER_INCH / boundingBoxHeight`. The inserted height in inches is therefore the "Default H" column: Circle and Square 2 in (5.08 cm), Triangle (down), Diamond, Line, Pennant and Half Circle 2.5 in, and so on. The width follows the aspect ratio. With no default height, a `CRICUT` image is scaled by 0.28 instead.
- **Position.** `setInsertPosition` (`main.js:145960-146030`) places the shape at viewBox origin + `DEFAULT_RULER_HEIGHT` (23, `main.js:82573`) + an offset that grows by `DEFAULT_INSERT_OFFSET_AMOUNT` (36 units = 0.5 in, `main.js:82430`) on each insert and resets when it passes the view size.
- **Colour.** The shape keeps its stored `layerFill.fillSolidColor`, which is `#000000` for every shape except the Line (`#4A494A`, a Score line; stroke `#000101`, width 1). `fillSolidColorOriginal` is `#494949`. The insert path has no recolouring code. Newer DS versions assign a colour per material in some flows; I did not find that here.

## 4. Operation dropdown

Paths below are relative to the scratchpad: `pretty/` is the pretty-printed bundle, `asar/` the extracted app. `en_US.json` means `asar/static-asset/design3/translation/en_US.json`.

### Component

- Selector `toolbar-operation-type`, class `r$` (exported as `n$`, static alias `wO`). Defined at `pretty/main.js:161227`; class body from `pretty/main.js:160977`.
- Inputs `withNewUi` (bool) and `state` (required). Output `updateLineTypeEvent`, which emits `{ outputType }` (`pretty/main.js:161146`).
- Component style: `.withNewUi{flex-grow:1;min-width:0} .canvas-operation-type__button{width:100%}` (`pretty/main.js:161515`).
- The trigger has two variants (`pretty/main.js:161464`). With `withNewUi` it is a `patch-flyout-trigger` button with `caretAlignment="down"` and class `canvas-operation-type__button`. It holds a `patch-icon` (cut/draw/print/guides) and the label (`Z$e`, `pretty/main.js:133585`). The legacy variant is `cr-edit-value-group-poc__dropdown-toggle` with a `span.caret` (`iKe`, `pretty/main.js:133618`).
- The trigger is disabled when `state.isLineTypeDisabled || !state.isOperationTypeModifiable` (`pretty/main.js:133598`).
- Trigger label is `state.selectedOperationTitleKey`. When `state.isValidOutputTypeForMachineFamily` is false it is shown wrapped in brackets, `" [" + label + "] "` (`pretty/main.js:133580`).
- The trigger icon comes from `operationIcon()` (`pretty/main.js:161062`). Cut group gives `"cut"`, draw group gives `"draw"`, PRINT/PRINTCUT give `"print"`, GUIDE gives `"guides"`.

### Groups (string output types)

From `pretty/main.js:161017-161032`:

```js
cutOperationTypes   = ["CUT", "WAVE", "PERF_BASIC"]
drawOperationTypes  = ["DRAW","MIDAS_FINE","MIDAS_REGULAR","MIDAS_BOLD","SCORE","DEBOSS_FINE","ENGRAVE"]
midasOperationTypes = ["MIDAS_FINE","MIDAS_REGULAR","MIDAS_BOLD"]
guidesOperationTypes = ["GUIDE"]
printOperationTypes = DH_PRINT_OPERATION flag ? ["PRINT","PRINTCUT"] : ["PRINTCUT"]
```

### Menu order, labels and i18n keys

The top-level template is at `pretty/main.js:161464-161512`. The sub-templates are `oKe` (cut, :133786), `_Ke` (draw, :134165), `gKe` (divider, :134203), `jKe`/`vKe` (Print, :134250/:134206), `bKe` (Print Then Cut, :134256) and `yKe` (Guide, :134304).

| # | Row | Output type | i18n key | en_US label | e2e id |
|---|---|---|---|---|---|
| H | header + `patch-icon cut` | | `COMMON_CUT` | Cut | |
| 1 | item | `CUT` | `COMMON_BASIC` | Basic | CanvasToolbarOperationType-CutBasic-Button |
| 2 | item | `WAVE` | `OPERATION_DROPDOWN_WAVY` | Wavy | ...-CutWave-Button |
| 3 | item | `PERF_BASIC` | `OPERATION_DROPDOWN_CUT_PERFORATE` | Perforate | ...-CutPerf-Button |
| | `patch-menu-divider` then header + `patch-icon draw` | | `COMMON_DRAW` | Draw | |
| 4 | item | `DRAW` | `MAT_CUT_HEAD_NAME_PEN` | Pen | ...-DrawPen-Button |
| 5 | item, or sub-menu trigger when >1 foil type | `MIDAS_*` | `COMMON_FOIL` | Foil | ...-DrawFoil-Button |
| 5a | sub-item | `MIDAS_FINE` | `MIDAS_FINE` | Fine | CanvasToolbarOperationType-Fine-Button |
| 5b | sub-item | `MIDAS_REGULAR` | `MIDAS_REGULAR` | Medium | ...-Medium-Button |
| 5c | sub-item | `MIDAS_BOLD` | `MIDAS_BOLD` | Bold | ...-Bold-Button |
| 6 | item | `SCORE` | `COMMON_SCORE` | Score | ...-DrawScore-Button |
| 7 | item | `DEBOSS_FINE` | `OPERATION_DROPDOWN_DRAW_DEBOSS` | Deboss | ...-DrawDeboss-Button |
| 8 | item | `ENGRAVE` | `COMMON_ENGRAVE` | Engrave | ...-DrawEngrave-Button |
| | `patch-menu-divider` (`showDividerAbovePrintCut`) | | | | |
| 9 | item, behind the `DH_PRINT_OPERATION` flag and Gemini/Scout project types only | `PRINT` | `COMMON_PRINT` | Print | ...-Print-Button |
| 10 | item + `patch-icon print` | `PRINTCUT` | `COMMON_PRINT_THEN_CUT` | Print Then Cut | ...-PtcStandard-Button |
| 11 | item + `patch-icon guides` (divider above in the screenshot) | `GUIDE` | `GUIDE_LABEL` | Guide | ...-Guide-Button |

The label keys and values are in `en_US.json`: `COMMON_BASIC` at 1201, `OPERATION_DROPDOWN_*` at 4596-4618, `MIDAS_*` at 3320-3322, `COMMON_CUT` at 1255, `COMMON_DRAW` at 1271, `COMMON_PRINT_THEN_CUT` at 1410, `GUIDE_LABEL` at 2075.

The foil sub-menu tooltip title is `translate("COMMON_FOIL") + ", " + translate(type)` (`pretty/main.js:133971`). The foil sub-menu e2e id is built in `getSupportedMidasLineTypes` (`pretty/chunk-JVMBAEXI.js:205304`): `MIDAS_REGULAR` becomes `Medium`, otherwise it is the capitalised suffix.

Foil rendering rule (`pretty/main.js:134188`):
- `supportedMidasLineTypes.length === 1` renders a single "Foil" item that sets `supportedMidasLineTypes[0].type`.
- More than one renders a "Foil" item that opens a `patch-menu` sub-menu (the right caret in screenshot 2.png).
- Zero hides the row.

Rich tooltip descriptions are keyed by `TOOLTIP_KEY_MAP` (`pretty/main.js:160981`). The keys are CUT CutBasic, WAVE CutWave, PERF_BASIC CutPerf, DRAW DrawPen, MIDAS_* DrawFoilFine/Medium/Bold, SCORE DrawScore, DEBOSS_FINE DrawDeboss, ENGRAVE DrawEngrave, PRINTCUT PtcStandard, GUIDE Guide and PRINT PrintOnly. The description strings are in `en_US.json:4597-4617`; one example is `OPERATION_DROPDOWN_CUT_BASIC_DESCRIPTION` = "Cuts your design with clean, solid edges.". They are only shown when the flag `DH_CANVAS_TOOLTIPS_V2` is on.

### Trigger label (selected operation)

The trigger label comes from `determineSelectorText(outputType, supportedMidas)` at `pretty/chunk-JVMBAEXI.js:205315-205358`.

```js
if (!t?.length) return "OPERATION_DROPDOWN_CUT_BASIC";            // "Basic Cut"
MIDAS_*  -> supportedMidas.length===1 ? "COMMON_FOIL" : OPERATION_DROPDOWN_FOIL_FINE|_REGULAR|_BOLD  // Fine Foil / Medium Foil / Bold Foil
CUT -> OPERATION_DROPDOWN_CUT_BASIC ("Basic Cut")   PRINTCUT -> OPERATION_DROPDOWN_CUT_PRINT_THEN_CUT
PRINT -> COMMON_PRINT   DRAW -> MAT_CUT_HEAD_NAME_PEN ("Pen")   SCORE -> COMMON_SCORE   ENGRAVE -> COMMON_ENGRAVE
DEBOSS_FINE -> OPERATION_DROPDOWN_DRAW_DEBOSS   DEBOSS_BROAD -> COMMON_DEBOSS   GUIDE -> GUIDE_LABEL
PERF_BASIC -> OPERATION_DROPDOWN_CUT_PERFORATE  PERF_STITCH -> COMMON_PERF ("Perf")
WAVE -> OPERATION_DROPDOWN_CUT_WAVY ("Wavy Cut")   MULTIPLE | CUTPRINTCUT -> COMMON_MULTIPLE ("Multiple")
```

The selection's line type is decided in `canvas-webw-composition.js`, offset ~302111 (minified):

```js
determineLineType(t,e,r){return 1===t.length&&"CUT"===t[0]&&r&&2===e.size?"CUTPRINTCUT":t.length>1?"MULTIPLE":1===t.length?this.isCutType(t[0])&&r?"PRINTCUT":t[0]:"CUT"}
```

Here `t` is the set of output types in the selection and `r` is true when there is a print fill. A cut layer with a print fill therefore becomes PRINTCUT.

### Per-item enable and selected rules (`pretty/main.js:133640-134350`)

- Every item: `disabled = !isLayerOperationAvailable(type)`. `isLayerOperationAvailable` returns true when `state.availableLayerOperations` is empty or contains the type (`pretty/main.js:161164`).
- CUT and WAVE are additionally disabled when `!state.selectionSupportsCutOperation`.
- `selected = state.selectedOutputType === type`. CUT is also selected for `CUTPRINTCUT`. Foil is selected when `state.isMidasSelected`.
- Each group (and each item) is shown only if `isSupportedArtTypeByMachine(type)` (`pretty/main.js:161149`). That calls `canvasUiAdapter.isLayerOutputTypeSupportedBySelectedMachineFamily`, which resolves to `selectedMachineFamily.supportedArtTypes.indexOf(type) > -1` (`pretty/main.js:421150`, `pretty/chunk-JVMBAEXI.js:164032`). With the `DH_OPERATION_FILTERING_BY_PROJECT_TYPE` flag it is also intersected with `getProjectTypeCapabilities(projectTypeId).operationTypes`.
- Guide is hidden when project-type filtering is active (`ce(i.shouldFilterArtTypesByProjectType() ? -1 : 9)`).
- Validity of the current type for the machine is decided by `determineValidOutputTypeForMachineFamily` (`pretty/chunk-JVMBAEXI.js:205362`). CUPID_FAMILY plus PRINTCUT is always valid, and CUTPRINTCUT is treated as PRINTCUT.

### Per-machine availability

`supportedArtTypes` per family comes from the bundled `asar/static-asset/design3/machines.2.json`. The live app may overwrite it from the API. This bundled copy has no MIDAS entries, so Foil would be hidden.

| machineFamilyType | name | supportedArtTypes |
|---|---|---|
| MAKER_FAMILY | Maker | CUT, SCORE, DRAW, PRINTCUT, ENGRAVE, DEBOSS, DEBOSS_FINE, DEBOSS_BROAD, PERF_BASIC, PERF_STITCH, WAVE |
| EXPLORE_FAMILY | Explore | CUT, SCORE, DRAW, PRINTCUT |
| CUPID_FAMILY | Cupid (Joy) | CUT, DRAW |
| MORPHEUS_FAMILY | Morpheus | CUT, DRAW |

The whitelist of all accepted tools is `AT.supportedTools` (`pretty/chunk-JVMBAEXI.js:164005`). It lists CUT, SCORE, DRAW, PRINTCUT, ENGRAVE(+_FILL/_OUTLINE/_PHOTO), DEBOSS_FINE, DEBOSS_BROAD, PERF_BASIC, PERF_STITCH, WAVE, MIDAS_FINE/REGULAR/BOLD, GUIDE(+_FILL/_OUTLINE), BLANK, PACKAGE, PRINT, CUT_DIE and CUT_KISS.

The screenshot (Maker 3) shows Foil with a sub-menu. The live family list therefore includes MIDAS types, which the bundled JSON lacks. I could not find the live per-machine list in the asar.

### Mapping: output type to art type (`Sn`), tool and layer enum

`Sn` art type enum is at `pretty/chunk-JVMBAEXI.js:140371`; the tool enum `Ft` is at `pretty/chunk-JVMBAEXI.js:140347`. The string-to-art-type switch `_getActualPathType` is at `pretty/main.js:353315` (`Kn` is the same enum).

The layer-operation numeric map is `yL`/`TL` (`pretty/chunk-JVMBAEXI.js:170424-170470`), used in the protobuf. The tool-name-to-`Ft` switch is at `pretty/chunk-JVMBAEXI.js:143140-143195`. The `ToolType` constructed for the WASM is in `convertOutputTypeToToolType` (`canvas-webw-composition.js` ~302300).

| Menu | Output type | Layer op id (TL) | Art type (Sn) | Tool (Ft) | WASM ToolType |
|---|---|---|---|---|---|
| Basic | CUT | 0 | CUT_ART_TYPE = 1 | ROLLING_BLADE_TT 1 / DRAG_KNIFE_TT 2 / TANGENTIAL_BLADE_TT 3 (selectedBlade) | Cut("drag") |
| Wavy | WAVE | 15 | WAVE_ART_TYPE = 6 | WAVE_TOOL_TT 11 | Wave |
| Perforate | PERF_BASIC | 19 | PERFORATE_ART_TYPE = 8 | PERFORATION_WHEEL_TT 9 | Perf("basic") |
| (Perf stitch) | PERF_STITCH | 18 | PERFORATE_ART_TYPE = 8 (enum also has PERFORATE_STITCH_ART_TYPE 13) | PERFORATION_WHEEL_TT 9 | Perf("stitch") |
| Pen | DRAW | 2 | DRAW_ART_TYPE = 2 | PEN_DT_TT 4 | Draw |
| Foil Fine | MIDAS_FINE | 21 | MIDAS_FINE_ART_TYPE = 10 | MIDAS_FINE_TOOL_TT 13 | Midas(fine) |
| Foil Medium | MIDAS_REGULAR | 22 | MIDAS_REGULAR_ART_TYPE = 11 | MIDAS_REGULAR_TOOL_TT 14 | Midas(regular) |
| Foil Bold | MIDAS_BOLD | 23 | MIDAS_BOLD_ART_TYPE = 12 | MIDAS_BOLD_TOOL_TT 15 | Midas(bold) |
| Score | SCORE | 1 | SCORE_ART_TYPE = 4 | STYLUS_TT 5 / SCORING_WHEEL_TT 6 / SCORE_TIP_TOOL_TT 101 (selectedScoreTool) | Score |
| Deboss | DEBOSS_FINE | 17 | DEBOSS_ART_TYPE = 5 | DEBOSSING_TOOL_TT 10 | Deboss(fine) |
| (Deboss broad) | DEBOSS_BROAD | 20 | DEBOSS_ART_TYPE = 5 (enum also has DEBOSS_BROAD_ART_TYPE 14) | DEBOSSING_TOOL_TT 10 | Deboss(broad) |
| Engrave | ENGRAVE | 16 | ENGRAVE_ART_TYPE = 7 | ENGRAVE_TOOL_TT 12 / ENGRAVE_STATIC_TOOL_TT 102 | Engrave |
| Print Then Cut | PRINTCUT | 3 | CUT_ART_TYPE = 1 for the cut path; the print layer is PRINT_ART_TYPE = 3 | selectedBlade | Cut("drag") |
| Print | PRINT | 34 | PRINT_ART_TYPE = 3 | none | None |
| Guide | GUIDE | 24 | none (filtered out of mat instructions, `pretty/chunk-JVMBAEXI.js:229492`) | none | Guide |
| (die/kiss) | CUT_DIE / CUT_KISS | 33 / 32 | CUT_DIE_ART_TYPE 15 / CUT_KISS_ART_TYPE 16 | selectedBlade | Cut("die"/"kiss") |

`_getActualPathType` throws for PRINTCUT, PRINT and GUIDE. PRINTCUT's cut path uses CUT_ART_TYPE (the `Sn` to "cut" map is at `pretty/chunk-JVMBAEXI.js:162674`). The PRINT_ART_TYPE = 3 assignment for the print layer is inferred, not read from code.

Plot order on the machine (`canvas-webw-composition.js` ~298260):

```js
_outputTypeToPlotOrder={SCORE:0,DRAW:1,DEBOSS_FINE:2,DEBOSS_BROAD:2,ENGRAVE:3,MIDAS_FINE:4,MIDAS_REGULAR:4,MIDAS_BOLD:4,PERF_BASIC:5,PERF_STITCH:5,WAVE:6,CUT:7,CUT_DIE:7,CUT_KISS:7,GUIDE:8,PRINT:11,PRINTCUT:11}
```

### Not found

- The live per-machine `supportedArtTypes` (with MIDAS) is not in the asar. It comes from the API.
- The en_US source has no `OPERATION_DROPDOWN_DRAW_ENGRAVE` or `_SCORE` label keys; the menu uses `COMMON_ENGRAVE` and `COMMON_SCORE`.

## 5. Edit toolbar

### Component

The edit toolbar is `canvas-ui-toolbar-v2`. Its class is at `pretty/main.js:166275`, its decorator at `:166551`, its template at about `:166953` and its styles at about `:167193`. The legacy `canvas-ui-toolbar` (`:164273`) is the older one. Every item is a `discrete-icon-button` with a `patch-icon` (20px) and a 14px label. Each one opens a `patch-popover` ("poptart") anchored below the toolbar, with `belowPopoverOffset = 3`. Items that do not fit move into a "{{count}} More" overflow menu (`CANVAS_MENU_EDIT_MORE_WITH_COUNT`; `moreButtonWidth` is 120, or 150 when undo/redo is in the toolbar).

### Item order

`bYe` (`main.js:137118`) renders the items only when `somethingIsSelected()`, which means `selectedGraphicIDs.size > 0`. Overflow ids are at `main.js:136492-136500`.

| # | Overflow id | Label (i18n key, en_US) | patch icon | data-test-id | Disabled when | Template fn |
|---|---|---|---|---|---|---|
| 0 | (optional) undo/redo | (icons only) | | | flag `DH_CANVAS_UNDO_REDO_IN_TOOLBAR` | `aYe` :136503 |
| 1 | `operationColorPicker` | operation dropdown + colour swatch (`toolbar-line-type-fill-type`) | | `CanvasToolbar-LineTypeFillType-Component` | `operationToolState().isLineTypeDisabled` | `oYe` :136525 |
| (text only) | `fontToolsGroup` / `fontFormat` | Font / Font Size / Font Style / Format | `advanced` | `CanvasToolbar-Format-Button` | nothing selected | `AYe`, `pYe` |
| 2 | `size` | Size (`CANVAS_MENU_EDIT_SIZE`) | `enlarge` | `CanvasToolbar-Size-Button` | `isNothingSelected()` | `dYe` :136749 |
| 3 | `flip` | Flip (`CANVAS_MENU_EDIT_FLIP`) | `flip-horizontal` | `CanvasToolbar-Flip-Button` | `flipState()` = `!isMirrorAvailable` | `uYe` :136795 |
| 4 | `arrange` | Arrange (`CANVAS_MENU_EDIT_ARRANGE`) | `drag-multi` | `CanvasToolbar-Arrange-Button` | `arrangeState().arrange` = no renderable group selected, or the selection is locked (flag `DH_SUPPORT_LOCK_FUNCTIONALITY_TOOLBAR_UPDATE`) | `hYe` :136841 |
| 5 | `align` | Align (`CANVAS_MENU_EDIT_ALIGN`) | `align-left` | `CanvasToolbar-Align-Button` | `alignState().align` = `!isAlignAvailable` | `fYe` :136887 |
| 6 | `rotate` | Rotate (`CANVAS_MENU_EDIT_ROTATE`) | `angle` | `CanvasToolbar-Rotate-Button` | `rotateState().isDisabled` = no renderable group, or locked | `mYe` :136933 |
| 7 | `effects` | Effects (`COMMON_EFFECTS`) | `wand-2` | `CanvasToolbar-Effects-Button` | `isNothingSelected()` | `gYe` :136982 |
| 8 | `combine` | Combine (`CANVAS_SIDEBAR_RIGHT_COMBINE`) | `shape-combine` | `CanvasToolbar-Combine-Button` | `!(combine \|\| flatten \|\| unflatten)` from `canvasLayerOperationToolsEnabledState` | `vYe` :137032 |
| 9 | `contour` | Contour (`CANVAS_MENU_ACTIONS_CONTOUR`) | `contour` | `CanvasToolbar-Contour-Button` | `contourToolState().isDisabled` = `!isContourAvailable` | `jYe` :137090 |

The state streams are in `pretty/chunk-JVMBAEXI.js`:
- `getFlipIsDisabledStream` :206902
- `getArrangeDisabledStatesStream` :206916
- `getAlignDisabledStateStream` :206956
- `getRotationToolStateStream` :207006
- `getLayerOperationsSidebarIsEnabledStream` :206606
- the contour mapping at `main.js:145444`

The `is*Available` booleans are computed inside `static-asset/wasm/CriCanvas.wasm`. `canvas-webw-cricanvas.js` only passes them through (for example `isAlignAvailable:e.isAlignAvailable`), so the exact thresholds are **not visible in JS**.

**Single-selection behaviour.** This comes from screenshot 2, where one shape is selected. Align, Combine and Contour render disabled, at 40% opacity (`.canvas-toolbar-v2__item--disabled{opacity:.4}`; the patch disabled style on buttons looks the same). Size, Flip, Arrange, Rotate and Effects stay enabled. Inferred rules:
- Align and Distribute need 2 or more selected items (Distribute needs 3).
- Combine (Weld, Unite and so on) needs 2 or more.
- Contour needs one layer with more than one contour.

These thresholds match the observed state but sit in the WASM, so treat them as inferred.

### Toolbar styling

From the v2 styles string:
- `.canvas-toolbar-v2`:
  - Graphik 400, 10px/16px.
  - `background:#fff; border-bottom:1px solid #cccccc`.
  - `height:40px; min-height:40px; max-height:50px; padding:4px; box-sizing:border-box`.
- Trigger buttons:
  - `color:#121212; padding:4px 8px; border-radius:8px`.
  - Label 14px/400. Icons 20x20.
  - Hover and active background `#e6e6e6`.
- Divider: a 1px x 16px line, `background-color:#adadad`, margin `0 4px`. The last item's divider is hidden.
- More button: `padding:8px 12px; border-radius:8px`, 12px/16px label, `chevron-down` icon at 16px. Active background `#ccc`. Focus ring `0 0 0 2px #0063b8`.
- More menu: `min-width:160px; padding:16px 0`. Rows are 32px. The active row has `background:#ebfaf5; color:#00855d`.
- Popover content: `.canvas-toolbar-v2__poptart{padding:16px}`. Title: Graphik 500, 14px/20px, `#121212`, `margin-bottom:16px`.

### Colour swatch picker

The swatch is part of `toolbar-line-type-fill-type` (`main.js:163936`). `.canvas-toolbar-operation-art-type__swatch` is 19x19px with `border-radius:8px`; when disabled it has `opacity:.5`. The trigger has `margin-top:7px`.

The popover is `toolbar-color-palette` (`main.js:150734`). Its sections are:
1. The current material colours (`colorPalette.materialColors`), in a scrolling section with max height 101px.
2. Basic colours (`defaultColors`).
3. With the new UI, a `patch-accordion` titled `CANVAS_MENU_EDIT_ADVANCED` ("Advanced") that holds an `ngx-color-picker` with hex output.

Layout and swatch size:
- Width: `--toolbar-color-palette-width`, 170px by default (`wW = {defaultView:"170px", fullWidthView:"100%"}`, `main.js:150697`).
- Grid: `repeat(auto-fill,18px)`, gap 5px, `padding:3px`, `margin-bottom:15px`.
- Swatch size: `x-small` (`small` in full width).

There are 28 basic colours, in this order (`class xO`, `main.js:150573`; duplicated as `Nmt`, `main.js:249379`). In a 170px grid of 18px tracks they lay out 7 per row:

```
#ef9999 #cf93da #9da8db #90caf8 #a5d6a7 #fef59c #ffcc80
#f53636 #9928b1 #3f51b5 #2296f3 #4daf50 #ffec3c #ff9702
#b71b1b #49148b #1b237e #0e47a0 #1d5e21 #fac02e #e65002
#ffffff #cccccc #999999 #666666 #333333 #000000 #8E5609
```

### Size popover (`toolbar-size`, `main.js:159832`; constants `:159613-159616`)

The new-UI template is `I$e`. In order, the row holds:
1. `patch-input-group` with prefix `COMMON_WIDTH_ABBREV` ("W") and a number input (`Toolbar-Width-Input`, spinners on).
2. A lock icon button (`Toolbar-Lock-Icon`, title `COMMON_LOCK_PROPORTIONS` "Lock proportions", icon `lock` or `unlock` from `state.isAspectRatioLocked`).
3. `patch-input-group` with prefix `COMMON_HEIGHT_ABBREV` ("H") and a number input (`Toolbar-Height-Input`).

Constants and behaviour:
- `step = 0.01`, `MIN_SIZE = 0.001`, `MAX_SIZE = 230` (in display units).
- Values commit on blur.
- Spinner ticks emit `{isTickEvent, isIncrement, isShiftKeyPressed}`. Shift clamps to the min or max.
- Text auto-selects on focus, throttled to 10 ms.
- CSS: `.toolbar-size__inputs{display:flex;gap:2px}`, width field 100px.

The popover title is not shown in v2 (`displayLabel: true` feeds the component's own label).

### Rotate popover (`toolbar-rotate`, class `z$` at `main.js:158309`, selector `:158404`)

The v2 toolbar uses `variant="slider-input"` (template `e$e`, `main.js:132116`) under the title `CANVAS_MENU_EDIT_ROTATE` ("Rotate").

The slider is a `patch-slider` with `min -180`, `max 180`, `step 1` and a tick mark at `[0]` (`C$`, `:130733`). Beside it, a number input (83px wide) with an `angle` icon prefix, `step 1`, `data-test-id` `CanvasToolbar-Rotate-Input`.

Mapping and behaviour:
- Slider to angle: `angle = s < 0 ? s + 360 : s`. Angle to slider: `a > 180 ? a - 360 : a`.
- Pasting "360" becomes 0.
- Input changes are debounced by 500 ms (`$nt`).
- Tick buttons step by plus or minus 1.
- The state angle is `selectedRotationDegrees` rounded to 2 decimals.

### Other popovers (for completeness)

- **Flip.** `CANVAS_MENU_EDIT_FLIP_HORIZONTAL` / `..._VERTICAL` (icons `flip-horizontal` / `flip-vertical`). Menu `min-width:160px; padding:16px 8px`.
- **Arrange.** `toolbar-arrange-menu` with `variant="menu-v2"`: Move to Front, Move Forward, Move Backward, Move to Back, plus the position fields.
- **Align.** `toolbar-align-grid`. Seven align options, in the order top, left, vertical, horizontal, bottom, right, centre (`main.js:154245`), then distribute vertical and horizontal.
- **Effects.** `toolbar-effects-menu` (`:164727`): Offset (`offset-outer`), Sticker (`sticker`), Warp (`warp`), Curve (`text-curve`, text only).
- **Combine.** `toolbar-combine-operations` (`:165208`), using icons `shape-slice`, `shape-weld`, `flatten-up`, `flatten-down`, `shape-unite`, `shape-subtract`, `shape-intersect`, `shape-difference` and `reset` (undo booleans).

The layers toggle at the far right of screenshot 2 is outside `canvas-ui-toolbar-v2`'s item list (input `isLayersHideShowBtnVisible`). I did not trace it further.

## 6. Project Preview and Mat Details

Paths are relative to the scratchpad. `composition.i18n.json` means `asar/design-space-app/static-asset/design3/translation-libs/canvas-composition/en_US.DesktopCanvasComposition.i18n.json`.

### Host and title

- The right split area of the canvas renders `div` > `cricut-visualization-sidebar` followed by `mat-visualization-panel` (`pretty/main.js:253063-253077`).
- The `mat-visualization-panel` is gated by `canDisplayMatVisualization()` (`pretty/main.js:260812`). That in turn calls `MatVisualizationService.canDisplayMatVisualization(projectType)` (`pretty/main.js:191472`):

  ```js
  let t = flag("DH_GRIZZLY_MAT_VISUALIZATION"); return !!e && t && yst.includes(e);
  // yst = [kd.VINYL_DECAL ("F301"), kd.IRON_ON_TSHIRT, kd.FOLDED_CARD]   (main.js:191233, kd at main.js:388575)
  ```

  In this bundle a Blank Canvas (`kd.BLANK_CANVAS = "F449"`) would therefore not show Mat Details. The live screenshot does show it for "Blank Canvas", so the server-side flag or allow-list differs from the bundled code. Treat "always show" as the target behaviour.
- `cricut-visualization-sidebar` is class `VB` (`pretty/main.js:249961`). Its header text is `translate("PREVIEW_PROJECT")`, which is "Preview Project" in the bundled en_US (`pretty/main.js:250033`).
- The screenshot reads "Project Preview". No key with that exact value exists in the asar; the only near match is `MAT_LOAD_FAILED_TITLE` = "Project Preview Could Not Load" at `en_US.json:3243`. The live string must come from remote translations.
- The close button is `patch-icon-button type=secondary data-test-id="ProjectVisualization-close-Button"` with `icon="x"`. The expand button is `ProjectVisualization-expand-Button` with `icon="enlarge"` (consts at `pretty/main.js:249990-250015`).
- Sidebar CSS (`vmt`, `pretty/main.js:246596`):
  - `--visualization-sidebar-width: 280px`
  - header `height:48px; padding:16px 16px 8px; color:#121212; background:#fff`
  - title `.text` is Graphik 400 at 16px
  - content box `280x280; margin:8px 16px; border-radius:8px`

  This 3D/colourisation preview area is not visible in screenshot 2.png. The live build seems to show only the header plus Mat Details.

### `mat-visualization-panel` (Mat Details accordion)

The component is at `pretty/main.js:192089`, with its template at `pretty/main.js:192130-192175`. The structure is:

```
div.mat-visualization__panel
  div.mat-visualization__accordionHeader (click toggleAccordion)
    h3.mat-visualization__title > span.mat-visualization__title-text {MAT_VISUALIZATION_TITLE}  + patch-icon icon="info" size=16 tooltip MAT_VISUALIZATION_DETAILS_TOOLTIP_TEXT
    div.mat-visualization__accordionHeader-controls
      div.mat-visualization__count > patch-badge-counter {matsList().length}
      patch-icon.mat-visualization__accordionHeader-toggle icon = open ? "minus" : "plus", size 16
  div.mat-visualization__accordionContent(.expanded)
    loading -> mat-visualization-cards-skeleton (default matsAmount 2)
    empty   -> img static-asset/design3/assets/canvas-composition/mat-visualization-nothing-to-show.svg + MAT_VISUALIZATION_NOTHING_TO_PREVIEW_MESSAGE
    else    -> div.mat-visualization__list > mat-visualization-card [mat] [loadNumber]=index+1 (cards separated by .mat-visualization__spacer)
    banner (warning) when compositionResult != Success
```

Strings are in `composition.i18n.json:94-107`:
- `MAT_VISUALIZATION_TITLE` = "Mat Details"
- `MAT_VISUALIZATION_LOAD` = "Load"
- `MAT_VISUALIZATION_DETAILS_TOOLTIP_TEXT` = "See how your design will cut — each mat represents a separate load. During Make, you can adjust materials, copies, and placement."
- `MAT_VISUALIZATION_NOTHING_TO_PREVIEW_MESSAGE` = "Nothing to preview yet. Add a few things to your Canvas to get started."
- `MAT_VISUALIZATION_PARTIAL_ERROR_TITLE` = "We can’t show everything right now"
- `MAT_VISUALIZATION_TOTAL_ERROR_TITLE` = "Something went wrong"
- `MAT_VISUALIZATION_TOO_COMPLEX_TITLE` = "Too much good stuff to show"

Panel CSS (`pretty/main.js:192178`):
- `.mat-visualization__panel{display:flex;flex-direction:column;margin:16px;flex:1 1 0;border-top:1px solid #e6e6e6}`
- `.mat-visualization__accordionHeader{display:flex;align-items:center;justify-content:space-between;padding:10px 0;cursor:pointer}`
- `.mat-visualization__accordionHeader-controls{gap:10px}`
- `.mat-visualization__title{font-family:Graphik,Arial,sans-serif;font-weight:500;font-size:14px;line-height:20px;gap:5px}`
- `.mat-visualization__list{display:flex;flex-direction:column;gap:8px;overflow-y:auto;padding:5px;transition:opacity .2s ease}`, with `--updating{opacity:.5;pointer-events:none}` while recomposing
- `.mat-visualization__spacer{border-top:1px solid #e6e6e6}`
- empty state `border:1px solid #cccccc;border-radius:8px;padding:16px 2px`, text Graphik 500 16px/24px `#595959`

The green "1" badge in the screenshot is `patch-badge-counter`. Its colour is defined in the patch design system and is not in this component's styles.

### `mat-visualization-card` (one per load)

The component is at `pretty/main.js:191630`, with its template at `pretty/main.js:191655-191700`:

```
div.mat-visualization__card(.selected) (click -> matSelected, opens mat-visualization-details-dialog)
  div.mat-visualization__matPreview > mat-preview [mat] maxWidth=100 maxHeight=100
  div.mat-visualization__info
    span.mat-visualization__info-load      "{{'MAT_VISUALIZATION_LOAD'|translate}} {{loadNumber}}"   -> "Load 1"
    div.mat-visualization__info-operations > div.mat-visualization__info-operation  {getOperationLabel(mat.operationTranslationKeys)}
    span.mat-visualization__info-matSize   {matSizeName}
```

`getOperationLabel` (`pretty/main.js:191608`) maps each key group through translate, joins the parts with `" "` and joins the groups with `", "`. For example `[["COMMON_BROAD","COMMON_DEBOSS"],["COMMON_SCORE"]]` gives "Broad Deboss, Score".

Card CSS (`pretty/main.js:191705`):
- `.mat-visualization__card{display:flex;flex-direction:row;border-radius:8px;overflow:hidden;padding:4px}`
- `:hover{box-shadow:0 3px 8px #12121233}`
- `.selected{border:1px solid #00855d}`
- `.mat-visualization__matPreview{width:120px;height:120px;padding:10px;align-items:center;justify-content:center}`
- the `mat-preview` gets `filter:drop-shadow(0px 1px 3px rgba(18,18,18,.2))`
- `.mat-visualization__info{font-family:Graphik,Arial,sans-serif;font-weight:400;font-size:14px;line-height:20px;color:#595959;flex-direction:column;gap:4px;justify-content:center}`
- `.mat-visualization__info-load{color:#121212;font-weight:700}`
- `.mat-visualization__info-operations{-webkit-line-clamp:3}`

**"On Mat".** The bundled card has only three text rows (Load, operations, size). The screenshot shows a fourth row, "On Mat". That label is `MAT_STYLE_MATTED` = "On Mat" (`composition.i18n.json`, `en_US.json:3249`). It comes from the mat style: `getMatStyleTitle` and `_getMatStyleTranslatedValue` (`pretty/main.js:184965`, `:188106`) return MATTED for `"mat"`, `MAT_STYLE_CARD_MAT` ("On Card Mat") for `"card"`, and `MAT_STYLE_MATLESS` ("Without Mat") for `"matless"`. Default mat style is `defaultMatStyle = "mat"` (`pretty/main.js:191434`). The live card evidently appends this style label as a fourth line, which this bundle lacks.

### Operation label per mat

Key table `pf` in `asar/design-space-app/canvas-webw-composition.js` (offset ~356600):

```js
pf={PRINTCUT:["OPERATION_DROPDOWN_CUT_PRINT_THEN_CUT"],PRINT:["OPERATION_DROPDOWN_CUT_PRINT_THEN_CUT"],CUT:["OPERATION_DROPDOWN_CUT_BASIC"],SCORE:["COMMON_SCORE"],DRAW:["MAT_CUT_HEAD_NAME_PEN"],DEBOSS_FINE:["OPERATION_DROPDOWN_DRAW_DEBOSS"],DEBOSS_BROAD:["COMMON_BROAD","COMMON_DEBOSS"],ENGRAVE:["COMMON_ENGRAVE"],MIDAS_FINE:["OPERATION_DROPDOWN_FOIL_FINE"],MIDAS_REGULAR:["OPERATION_DROPDOWN_FOIL_REGULAR"],MIDAS_BOLD:["OPERATION_DROPDOWN_FOIL_BOLD"],PERF_BASIC:["OPERATION_DROPDOWN_CUT_PERFORATE"],PERF_STITCH:["COMMON_STITCHING","COMMON_PERF"],WAVE:["OPERATION_DROPDOWN_CUT_WAVY"]}
```

`_getOperationTranslationKeys(mat.allOperationTypes)` dedupes the output types and sorts them with PRINTCUT first, then by `_outputTypeToPlotOrder` (SCORE 0, DRAW 1, DEBOSS 2, ENGRAVE 3, MIDAS 4, PERF 5, WAVE 6, CUT 7, GUIDE 8, PRINT/PRINTCUT 11). On Cupid or Mars machines MIDAS_REGULAR shows as "Foil". For a plain cut mat the label is "Basic Cut", as in the screenshot.

### Mat size label and units

- `getMatSizeName(mat)` (`pretty/main.js:191478`) finds the material whose `id === mat.materialSizeId` among `getAvailableMaterials` (sorted by width, `pretty/main.js:191294`) and returns its `name`.
- The names are pre-formatted strings from the machine definition, not computed. The worker stores `imperialName_i18n` as `name` and `metricName_i18n` as `metricName` (`convertAvailableMaterials`, composition worker ~341612).
- The 12x12 mat in `asar/static-asset/design3/machines.2.json` (MAKER/EXPLORE, `materialSizeID` "11", 864 x 864 pt, `isPreferred:true`) has `imperialName_i18n` `12" x 12"` and `metricName_i18n` `30.5 cm x 30.5 cm`. The others are id 12, `12" x 19"` / `30.5 cm x 48.26 cm`, and id 13, `12" x 24"` / `30.5 cm x 61 cm`.
- Internal units are points, 72 per inch (864 pt = 12 in).
- The bundle returns `name` (imperial). The screenshot shows metric, so the live build picks `metricName` when `MEASUREMENT_TYPE === "METRIC"` (the worker's `_isMetric` getter, ~311407). For the clone, use the metric string when units are cm and the imperial string otherwise.

### Mat thumbnail (mat-preview background)

These values are from the composition worker (~314449-322374):
- mat background colour `MAT_BACKGROUND_COLOR ?? "#afafaf"`, corner radius 20, with a notch cut-out for style "mat"
- grid primary interval `CANVAS_POINTS_PER_CENTIMETER` when metric, else `CANVAS_POINTS_PER_INCH`; the secondary interval is the other one
- rulers dark colour `"#333333d9"`
- material area fill is the mat's `fill.backingColor`

The black mat in the screenshot is the material fill colour (`backingColor`) of the layer colour.

### Splitting a design into mats

The "Sorting project into mats by color" label is `MAT_CUT_PREVIEW_SORTING_PROJECTS` (`en_US.json:3094`), shown as a loader label (`pretty/main.js:191009`, `:395045`, `:400439`).

The split itself is not in JS. `initializeCriComposition` calls `this._wasmService.CriCanvas.Composition.init(canvasState, matTemplateData, compositionMatStyle, {preferredMaterialSize, layoutFromMinXMaxY})` (`canvas-webw-composition.js` ~343965), so the grouping lives in `asar/static-asset/wasm/CriCanvas.wasm`. What the JS does show:

- Each resulting mat has one `fill` (a Material fill built from a single `CanvasColor`, see `createEmptyMat`: `new Fill.Material(CanvasColor.fromHex(t.fill))`). It also has `allOperationTypes`, `canvasState.containsPrintThenCut` and `isMirrored`. So one mat is one material colour, plus a separate mat type for Print Then Cut. PTC mats skip `changeFill`, and their grid draws on top of content.
- GUIDE and PRINT layers are excluded from machine steps (`buildMatInstructions`, `pretty/chunk-JVMBAEXI.js:229490`).
- Within a mat, tool steps are deduped by tool. Pens are deduped by tool plus `strokeColor` (`getRequiredSteps`, `pretty/chunk-JVMBAEXI.js:229563`). Steps are ordered by `YQ` (`pretty/chunk-JVMBAEXI.js:223107`), or by `zQ` for Bifrost devices: SCORE, DRAW, DEBOSS_FINE, DEBOSS_BROAD, ENGRAVE, MIDAS_FINE, MIDAS_REGULAR, MIDAS_BOLD, PERF_BASIC, PERF_STITCH, WAVE, CUT, PRINTCUT, CUT_DIE, CUT_KISS. Foil steps of different colours are sorted by `strokeColor`.

The following rule is inferred from those fields and from documented Design Space behaviour; it is not read from code. Group the visible, non-guide layers by (PTC or not, fill colour). Draw, score and foil layers attached to a cut layer join that cut layer's mat. Order mats with PTC first and then by colour, and number the loads 1..N in that order.

### Not found

- The exact WASM grouping and sort order of mats.
- The live "Project Preview" string key.
- The fourth "On Mat" row in the card template.
- The `patch-badge-counter` colour.

## 7. Canvas grid and rulers

There are two renderers in the bundle. The live design canvas uses the **CriCanvas** path. Its state factory is `HZ()` at `chunk-JVMBAEXI.js:202970`, the grid is drawn by `renderGridToCanvasContext` at `:199882`, the rulers by `_renderFullTopRuler` / `_renderFullLeftRuler` at `:200815` / `:200876`, and zoom is handled by `CanvasViewBoxService` at `:211983`.

The older `cds-canvas` worker path (`getRulerBitmaps` / `drawGrid` at `chunk-JVMBAEXI.js:193303-193530`, duplicated in `canvas-webw-render.js`) and the SVG `#ruler_x` / `#ruler_y` CSS (`main.js:263167`, 15px rulers) are legacy. Do not copy them. The screenshot matches CriCanvas: 24px ruler, labels every 10cm, 1cm grid.

### Units and scale

- Canvas units are points. `CANVAS_POINTS_PER_INCH = 72` and `CANVAS_POINTS_PER_CENTIMETER = 72 / 2.54` (`main.js:82472`, `:82475`).
- At 100% zoom, one point is one CSS pixel, multiplied by `devicePixelRatio` in device pixels. The multiplier code divides the transform scale by `devicePixelRatio` before comparing it with 72 (`chunk-JVMBAEXI.js:199846-199851`). The screenshot confirms this. 10cm spans 327 screenshot px, and 327 / 1.143 = 286 CSS px, against the expected 283.5.
- The default unit is `MEASUREMENT_TYPE = "IMPERIAL"` (`main.js:82606`). The user preference `systemOfMeasurement` is upper-cased and falls back to `"IMPERIAL"` (`main.js:284317-284320`).

### Grid

The grid state is `"FULL" | "PARTIAL" | "OFF"`. The default is `CANVAS_GRID_PREFERENCE = "PARTIAL"` (`main.js:82517`). It is overwritten by the user preference `canvasGridState`, which falls back to `"PARTIAL"` (`main.js:284315`, `:284333`).

The settings radios are `SETTINGS_FULL_GRID` 'Full Grid', `SETTINGS_PARTIAL_GRID` 'Partial Grid' and `SETTINGS_NO_GRID` 'No Grid', under `SETTINGS_CANVAS_GRID` 'Canvas grid' (`i18n/global-en_US.txt:5235`, `:5239`, `:5266`, `:5267`; radios at `main.js:181756-181768`). `cycleGridState()` (`chunk-JVMBAEXI.js:203806`) cycles OFF to FULL, PARTIAL to OFF, and FULL to PARTIAL. The screenshot shows FULL.

The colours depend on the canvas background (`chunk-JVMBAEXI.js:203786-203794`):

- On a light background, major lines are `#b8b8b8` (`MAJOR_GRID_DEFAULT_DARK_COLOR`) and minor lines are `#dbdbdb` (`MINOR_GRID_DEFAULT_DARK_COLOR`).
- On a dark background, major lines are `#ebebeb` and minor lines are `#919191` (`main.js:82524-82527`).
- The background is `#FFFFFF` (`main.js:82519`) and the line width is `CANVAS_GRID_LINE_WIDTH = 1` (`main.js:82549`).

Spacing logic (`renderGridToCanvasContext`, `chunk-JVMBAEXI.js:199882-199971`):

```
p    = hypot(viewTransform.a, viewTransform.b)          // device px per pt
unit = IMPERIAL ? 72 * p : (72/2.54) * 10 * p            // 1 in, or 10 cm
J    = unit * multiplier(unit)                          // major line pitch, device px
subdivisions = IMPERIAL ? 8 : 10                         // minor lines per major cell
start at viewTransform.e / .f, step J; skip the line at 0
FULL draws (subdivisions - 1) minor lines inside each major cell; PARTIAL draws majors only
```

`multiplier` is `_getFullMultiplierForZoomLevel` (`chunk-JVMBAEXI.js:199846-199852`). It computes `e = unit / devicePixelRatio`, which is the CSS-pixel size of 1in or 10cm. With `n` = 72 (imperial) or 283.46 (metric), it returns:

```
e < n/8 -> 16;  e < n/4 -> 8;  e < n/2 -> 4;  e < n -> 2;  else 1
```

At 100%, an imperial grid has a major line every 1in and, in FULL, minor lines every 1/8in. A metric grid has a major line every 10cm and minor lines every 1cm. When you zoom out, the major pitch doubles at each halving threshold.

### Rulers

The ruler is CriCanvas `ruler` state (`chunk-JVMBAEXI.js:202981-202991`):

| Field | Value | Source |
|---|---|---|
| width (thickness) | 24px CSS (`DEFAULT_CRICANVAS_RULER_HEIGHT`, `?? 24`) | `main.js:82574` |
| backgroundColor | `#f5f5f5` | `main.js:82528` |
| tickColor (also the 1px border) | `#b8b8b8` | `chunk-JVMBAEXI.js:202984` |
| numberColor | `#595959` | `main.js:82529` |
| imperial tick lengths | `[0, 0.825, 0.712, 0.825, 0.6, 0.825, 0.712, 0.825]` | `main.js:82609-82611` |
| metric tick lengths | `[0, 0.8, 0.8, 0.8, 0.8, 0.8, 0.8, 0.8, 0.8, 0.8]` | `main.js:82612-82614` |

Top ruler drawing (`_renderFullTopRuler`, `chunk-JVMBAEXI.js:200815-200874`). The left ruler mirrors it with rotated text.

```
r = devicePixelRatio; l = 24 * r (ruler thickness, device px)
fill whole ruler #f5f5f5; strokeRect 1px border in tickColor
unit = IMPERIAL ? 72*c : 283.46*c       // c = view scale
mult = multiplier(unit)  (table above)
h    = unit * mult                      // labelled interval in device px
labelStep = IMPERIAL ? 1 : 10           // the number shown increments by mult*labelStep
for each labelled position k:
  major tick: k, from y=l (bottom edge) up to y=V[0]=0, so full height (skipped for label 0)
  minor ticks: V.length-1 ticks at spacing (IMPERIAL ? 72/8 : 28.35) * c * mult,
               each from y=l up to y=l*V[z]
  label: `${A}` in `${12*r}px Arial`, textBaseline top, textAlign left, at (k + 4r, 4r)
```

Tick heights as a fraction of the 24px ruler, measured from the bottom edge, are `1 - V[z]`:

- **Imperial**, 8 ticks per inch. The 1/8in ticks are 17.5% (4.2px). The 1/4in ticks are 28.8% (6.9px). The 1/2in tick is 40% (9.6px). The inch tick is full height and carries the label.
- **Metric**, 10 ticks per labelled interval. Every 1cm tick is 20% (4.8px). The 10cm tick is full height. Labels read 0, 10, 20, 30 and so on in cm (`v = 10`, `A += mult * 10`). This matches the screenshot.

Both rulers draw their labels and ticks only from 0 upward. Negative positions are skipped (`if (A < 0) { A = 0; continue }`). The left ruler draws numbers rotated by -90 degrees with `textAlign "right"` (`chunk-JVMBAEXI.js:200925-200935`).

### Unit toggle (in/cm)

There is no ruler-corner click toggle in the CriCanvas path. The unit is set from canvas settings. `canvas-ui-settings` (`main.js:181663`) has a `gridUnits` setter (`main.js:181608-181625`). It calls `_rendererStateManager.setMeasurementType("METRIC" | "IMPERIAL")` and persists the choice through `setUserSystemOfMeasurementPreferences`. The labels are `SETTINGS_UNITS` 'Units', `SETTINGS_INCH` 'Inch' and `SETTINGS_CENTIMETER` 'Centimeter' (`i18n/global-en_US.txt:5277`, `:5240`, `:5236`). Changing the unit switches the grid subdivisions (8 or 10), the ruler label step (1in or 10cm) and the tick arrays.

### Zoom control "− 100% +"

- **Component.** The component is `canvas-ui-zoom` (`main.js:179713`). It has a minus button (`patch-icon minus`, 16px), the value, a plus button (`patch-icon plus`, 16px) and an optional zoom-to-fit button.
- **Value text.** The value is rendered as `{{ zoomValue | number:'1.0-0' }}%` (`main.js:142513-142523`). `zoomValue = zoomFactor * 100` (`main.js:144873`, `:145328`). Clicking the value emits `zoomReset`, which sets the factor to 1.
- **Pill style.** `min-width:155px`, `height:36px`, `border-radius:40px`, `border:1px solid #cccccc`, white bg, Graphik 400 14px/20px `#121212` (`main.js:179797`). It is positioned by `.zoom-ui{position:absolute;bottom:25px;left:45px}` (`main.js:263167`).
- **Default zoom.** `_zoomFactor = 1`, which is 100% (`chunk-JVMBAEXI.js:211983`). The legacy `INITIAL_ZOOM_VALUE = 60` (`main.js:82603`) is not used by CriCanvas.
- **Range.** `If = 0.1` to `Hv = 7`, that is 10% to 700% (`chunk-JVMBAEXI.js:211954-211955`; clamp in `setZoom` at `:212233` and `_findNearestThresholdFactor` at `:212483`). Legacy view-state limits agree: `zoomOutPercentageLimit:10`, `zoomInPercentageLimit:700` (`chunk-JVMBAEXI.js:176610-176611`). The legacy `MIN_ZOOM_VALUE 25` / `MAX_ZOOM_VALUE 700` is at `main.js:82604-82605`.
- **Button step.** `_getNextStepZoom` (`chunk-JVMBAEXI.js:212487-212518`) uses the constants `JF=0.25`, `KF=2`, `zP=0.05`, `$P=0.25`, `qP=0.5` and `Pf=0.001` (`:211948-211953`).
  - Below 25%, or at exactly 25% when zooming out, the step is 0.05.
  - At 200% and above when zooming in, or above 200% when zooming out, the step is 0.5.
  - Otherwise the step is 0.25.
  - The next value snaps to the nearest multiple of the step, strictly beyond the current value (`roundToNearest`, `chunk-JVMBAEXI.js:222925`). The result is rounded to 2 decimal places.
  - From 100%, the sequence in is 125, 150, 175, 200, 250, 300 … 700. The sequence out is 75, 50, 25, 20, 15, 10.
- **Wheel and trackpad.** These are legacy config values. I did not confirm that CriCanvas uses them.
  - `MOUSEWHEEL_DEFAULT_ZOOMIN 1.1` / `ZOOMOUT 0.9`
  - `TRACKPAD_DEFAULT_ZOOMIN 1.01` / `ZOOMOUT 0.99`
  - `ZOOM_IN_SCALE_RANGE [1, 1.03]`
  - `MOUSEWHEEL_DELTA_RANGE [-4, -50]`
  - `TRACKPAD_DELTA_RANGE [-0.1, -2.5]`
  - Source: `main.js:82651-82659`.

### Canvas default size

- `DEFAULT_CANVAS_SIZE = 252 * 72` = 18144pt, which is 252in square (`main.js:82575`). It is used as `canvasMaxSize` and as the ruler `stop` (`chunk-JVMBAEXI.js:176622-176634`).
- `CANVAS_SIZE = 13824`, which is 192in (`main.js:82480`). This is a second constant, and I did not trace where it is used.
- The canvas origin is the top-left. Rulers start at 0, and negative coordinates are not labelled.

### Not found

- I did not find the exact wheel-zoom handler for CriCanvas.
- I did not find a ruler-corner unit toggle. I believe it does not exist in the CriCanvas path.
- I did not find whether the scroll bounds clamp at 0. The "scrollbars" settings were not traced.

## 8. Tab bar and header

The window chrome has two stacked bars. The **title bar** (44 px) is a separate Angular app, `asar/design-space-tabs`, that contains the tabs and the account tools. The **canvas header** (64 px) belongs to `design-space-app` and contains the project title, undo/redo, save, My Stuff, the trial ad, Preview and Make. `prettytabs/tabs-main.js` is `asar/design-space-tabs/main.js` run through prettier 3.3.3.

### Title bar (`app-title-bar`, prettytabs/tabs-main.js:41920)

- Host style (tabs-main.js:41920 block):
  - Unfocused: `height:44px; padding-right:22px; --title-bar-background-color:#8f8f8f; --tab-background-color:#adadad; --tab-text-color:#adadad`.
  - Focused window (`.focused`): `--title-bar-background-color:#f5f5f5; --tab-background-color:#ffffff; --tab-text-color:#404040`.
  - Platform padding: `.osx{padding-left:88px}` leaves room for the traffic lights; `.win32{padding-right:0}`, and window controls are drawn by `app-window-controls`.
  - `.tabs-disabled:after` overlays `#00000026`.
- Tools row: `.title-bar-tools{display:flex;gap:16px;align-items:center}`.
- Text buttons: `.button{font:400 14px/20px Graphik,Arial,sans-serif;color:#121212;border-radius:4px;padding:0 6px;height:32px}`, hover `#e6e6e6`.
- Beta pill: `.badge{font:500 12px/16px Graphik;background-color:#0c80e4;color:#fff;padding:4px 8px;border-radius:4px}`, hover `#0063b8`. It shows only when `APPLICATION.TYPE === "BETA"` (`_determineBetaApplicationExperienceBadgeVisibility`, tabs-main.js:41893-41896).
- Item order, from `Z$` at tabs-main.js:41527:
  1. Help menu (`tabs-help-menu`, icon `help`, label default `"Help"` at tabs-main.js:40661).
  2. Log stats (debug).
  3. Bug submission (debug).
  4. Beta badge.
  5. Machine picker (`app-machine-connection-status-icon`, icon `machine`, plus `toolbarText` e.g. "Maker 3").
  6. Chat help (`app-help-menu-chat` with icon `help-assistant`, or `app-help-menu-chat-experiment` behind `isChatbotIconsExperimentEnabled$`, `B$` :41485).
  7. Notifications (`app-notifications`, icon `notification`).
  8. Shopping cart (only when `shouldShowShoppingCart$`).
  9. Profile (`app-user-profile-menu`: avatar + name + chevron-down).
- Machine picker style (tabs-main.js:40832 block): `.machine-picker-button{padding:4px 10px;border-radius:8px}`, hover `#ccc`. Status dot `8px` circle at `top:1px;right:0`: `#ba3003` when disconnected, `#006648` when connected (`--connected`). Family name `400 14px/20px Graphik; padding-left:4px`.
- Notification badge (tabs-main.js:40182 block): `background:#db3903; color:#fff; font 400 10px/16px Graphik; height:16px; min-width:16px; border-radius:8px; top:0; inset-inline-start:18px`. Shows `99+` above 99, and gets `padding:0 5px` above 9 (`e$`, tabs-main.js:40143).
- Profile (tabs-main.js:40255 block): `img{width:32px;height:32px;border-radius:50%}`, icon background `#e6e6e6`. Name is `400 14px/20px Graphik; margin-inline-start:16px; margin-inline-end:4px`.

### Tabs (`app-tabs`, prettytabs/tabs-main.js:40060)

- Template `QG` (:39973): `div.tab-bar > div.tab-section[role=menu]` contains a Home button, then either one Canvas button (`!multiTabsAllowed && isUserLoggedIn && isCanvasActive`) or a list of canvas tabs when multi-tab is on (`IS_MULTI_TABS_ENABLED = !1`, so off), then a `new-tab` "+" button (icon `plus`, size 12, hidden when multi-tab is off via `new-tab--hidden`).
- Home tab: icon `house-filled` (size 24, `$G` :39878) followed by `span.tab-section__home-tab-spacer.tab-section__tab-text` (homeTabName). Disabled when `tabDisplayOptions().disableHomeTab || !isOnline()` (:40025).
- Styles (:40132):
  - `.tab-section{height:40px;margin-top:4px;font-family:Graphik}`.
  - Buttons: `button{padding:0 24px;font:500 14px/20px Graphik;color:var(--tab-text-color)}`.
  - Home: `button.home{padding:0 15px;min-width:104px;justify-content:center;color:#00855d}`.
  - Active: `button.active{border-radius:4px 4px 0 0;background-color:var(--tab-background-color)}`, with `:before`/`:after` 10 px circles whose box-shadows (`13px 13px 0 10px` / `-13px 13px 0 10px`) draw the inverted corner flares.
  - Inactive hover: `#e6e6e6` with radius `4px 4px 0 0`.
  - Tab text: `.tab-section__tab-text{color:#121212}`.
  - Close `x` on canvas tabs is size 16 (:39963).

### Canvas header (`app-header-bar` inside `canvas-routing-home-design`, pretty/main.js:265315)

- Bar: `app-header-bar .header-bar{position:fixed;top:0;left:0;z-index:25;width:100%;background-color:#fff;height:64px;box-shadow:0 1px 3px #12121233}` (design-space-app/styles.css byte 189788).
- Content: `.header-bar__content{display:flex;justify-content:space-between;padding:12px 24px}` (styles.css byte 189944).
- The view below is offset to match: `.cr-view-container{left:70px;width:calc(100% - 70px);top:64px;height:calc(100% - 64px)}` (main.js:265993 styles).
- Template, main.js:265821:
  - Left (`header-bar__content__left`): project title block (`Ogt`/`Bgt`/`lvt`), then the optional copy-to-clipboard button, then the undo/redo block (`dvt`).
  - Right (`header-bar__content__right`): offline notice (icon `offline`, `COMMON_OFFLINE_MODE`), then save + My Stuff (`gvt`), warnings button, Braze marketing ad, Braze access ad (the "Start Free Trial" pill) or commerce ad, then `span.header-bar__content__right__buttons{display:flex;gap:10px}` holding Preview, Add to Cart (DTF only) or Make.
- Undo/redo: `toolbar-undo-redo` (`withNewUi`, icons `undo`/`redo`, main.js:157868) preceded by `.header-bar__content__left__undo-redo-divider{width:1px;height:24px;border-radius:8px;background-color:#ccc;margin:0 8px}` (main.js:265993 styles). It shows when `isToolbarV2Enabled() && !isUndoRedoInToolbar() && showHeaderUndoRedo()`.

#### Project title ("Blank Canvas", pencil, chevron)

The component is `canvas-routing-project-header`, pretty/main.js:254872, with its class at :254765.

- With `FEAT_DH_AUTO_SAVE_2026` on (`autoSaveEnabled`, :254794), view mode (`l_t`, :252159) renders `span.project-header__name` with `getDisplayName()`, an optional chevron-down (size 12, when `showDetailChevron`), and the `edit` pencil icon (size 16, class `project-header__edit-icon`).
- Clicking the name or the pencil calls `onNameClick()`, which enters edit mode (`s_t`, :252091). Edit mode shows a hidden sizer span, an `<input class="project-header__input">`, a `checkmark` confirm and an `x` cancel (both size 20). Confirm and cancel use `mousedown.preventDefault` so the input does not blur first.
- Keys (`onEditKeydown`, :254841): Enter calls `commitRename()`. Escape and blur call `cancelRename()`.
- `commitRename` (:254846): `name = editBuffer.trim() || originalName`. If the name differs from `canvasData.projectName`, it sets the name and calls `adapter.renameProject(name)`.
- Input width (:254862): `clamp(sizer.scrollWidth + 14, 20, 300)` px.
- Display name (`getDisplayName`, :254797): in order, the committed name, then `canvasData.projectName`, then `COMMON_UNTITLED_PROJECT_CANVAS_IQ` with `{projectTypeName}` when a project type is set, else `COMMON_UNTITLED_PROJECT` = 'Untitled Project' (global-en_US.txt:1512).
- Styles (:254985):
  - `.project-header{display:inline-flex;gap:4px}`.
  - Name: `.project-header__name{max-width:300px;font-size:13px;ellipsis}`.
  - Input: `.project-header__input{font-size:13px;border:2px solid var(--patch-color-border-accent-primary,#00855d);border-radius:4px;padding:2px 6px;height:32px;max-width:300px;background:#fff;color:#00855d}`, selection `#ebfaf5`.
  - Icons: `color:var(--patch-color-icon-secondary,#666)`, hover `#333`.
  - The header container adds class `patch-display-xs` (consts at main.js:265343).
  - `canvas-routing-project-header.is-editing ~ canvas-ui-project-instructions{display:none}` hides "Project Instructions" while renaming (main.js:265993 styles).
- "Blank Canvas" in the screenshot comes from the project type name. `COMMON_UNTITLED_PROJECT_CANVAS_IQ` is **not** in `i18n/global-en_US.txt` or `static-asset/design3/translation/en_US.json`, so its template text could not be confirmed. `PROJECT_TYPES_BLANK_CANVAS` = 'Blank Canvas' (global-en_US.txt:5007) is the likely project type name.
- Without the auto-save flag (`m_t`, :252237), the title shows `canvasData.projectName` with a click-to-show-details chevron. If no name is set, it shows `COMMON_UNTITLED_PROJECT_CANVAS_IQ` or an empty span.
- "Project Instructions" (`PROJECT_INSTRUCTIONS`, global-en_US.txt:4913) is a separate `canvas-ui-project-instructions` link rendered under the title (main.js:142234, with icon size 12).

#### Save state ("Saved" / "Save")

Branch selection is in `gvt` (main.js:253983):
- `autoSaveEnabled` uses the `canvas-routing-auto-save-menu` component (`hvt`).
- `saveOnly$` uses a single Save button (`fvt`, disabled by `saveOnlyDisabled$`).
- `saveAsOnly$` uses a Save button that opens Save As (`mvt`).
- Otherwise a "Save" link opens a menu with Save (disabled by `cannotSave$`) and Save As (`CANVAS_PROJ_SAVE_AS_BTN` = 'Save As').

`canvas-routing-auto-save-menu` is defined at main.js:263613.
- `saveStatus` comes from `adapter.saveStatus$` with initial value `"idle"` (:263591). The values are `idle | saving | saved | error`.
- The trigger is a `patch-link-button type=secondary`. Its leading icon is `download-cloud` (shown only when `autoSaveEnrolled()`). It gets class `auto-save-menu__cloud--error` and the trigger gets `auto-save-menu__trigger--error` when the status is `error`.
- Label by status (template :263719-263760):
  - `saving`: `CANVAS_AUTO_SAVE_SAVING` + "..." ('Saving...').
  - `saved`: `COMMON_SAVED` ('Saved').
  - `error`: `CANVAS_AUTO_SAVE_NOT_SAVED` ('Not Saved').
  - `idle`: `COMMON_SAVE` ('Save').
  - The keys are at global-en_US.txt:358, 1443, 353 and 1435.
- Tooltip by status (:263583-263590):
  - saving: 'Project saving to the cloud'.
  - saved: 'Project saved to the cloud'.
  - error: 'Project failed to save'.
- Menu items: Save (disabled by `cannotSave$`), "Save a copy" (when `showSaveAs`), a divider, and an "Auto Save" row (`CANVAS_AUTO_SAVE`) with an info icon (size 12) and a `patch-switch` bound to `autoSaveEnrolled`.

My Stuff (`gvt`, main.js:253983): `patch-link-button type=secondary`, icon `inbox`, text `MY_STUFF` = 'My Stuff' (global-en_US.txt:3682). Disabled when offline, and clicking does nothing when offline.

#### Start Free Trial

This is `braze-header-ad` with `template="access-button"` and `location="CANVAS"` (consts at main.js:265343). It shows when `isBrazeAccessAdEnabled$`, otherwise `commerce-header-ad` shows if `isNewCanvasAdEnabled$`. The defaults are in design-space-app/chunk-ACGDDC32.js byte 108938: `{bgColor:"#EBFAF5",textColor:"#00855D",iconLeft:"access-a",iconRight:"none",tmpl:"access-button"}`. The pill style is `.header-ad{padding:8px 16px;border-radius:8px;font-weight:400}`. The text ("Start Free Trial") and the ↗ icon come from the remote Braze card. `CRICUT_ACCESS_TRIAL_CTA` = 'Start Free Trial' (global-en_US.txt:1735) is the in-app equivalent.

#### Preview and Make buttons

Both are `patch-button size=medium`.
- Preview (`Cvt`, main.js:254057): `type=secondary`, text `COMMON_PREVIEW`. It renders only when `canShowRightPreview()`, which is `hasValidVariations() && hasArtboard() && !hideRightPanelForGemini()` (main.js:255193). **Disabled when `!isCanvasDirty()`.** `isCanvasDirty` traces to `_stateService` (main.js:246630). Dirty becomes true on the first insert (`_handleCanvasFirstTimeInsert`, :246842) or when artboard content exists (`_handleCanvasDirtyFromArtboard`, :246826). It resets to false on first load of a canvas (`_handleCanvasFirstTimeLoad`, :246833). All of these require `_hasValidProjectTypes()`. Clicking calls `openCanvasPreview()`.
- Make (`Mvt`, main.js:254095): primary type (default), text `PROJECT_DETAIL_MAKE_BTN` = 'Make' (global-en_US.txt:4723). **Disabled when `isGoEnabled$ === false`.** `isGoEnabled$` (main.js:264188) combines the design-area experiment, the worker canvas state and `isLoading$`. It is true when `canvasContainsNonIgnorableGraphics(isLoading)`, which is false while loading or while prepping mat preview (:264431). With the design-area experiment on, it also requires that the artboard is not empty. Clicking calls `goToMat()`.
- For DTF projects (`isDtfEnabled`), Make is replaced by "Add to cart" (`Svt`, icon `cart`) with the same `isGoEnabled$` gate.
- Button tokens (patch-button in design-space-app/chunk-GJSEQL7X.js byte 1289599):
  - Base: `border-radius:9999px; min-height:32px; min-width:132px; max-width:320px`.
  - Medium: `font:500 14px/20px Graphik; line-height:1; padding:8px 24px; height:40px; gap:8px`.
  - Primary: `#00855d` bg, `#fff` text, hover `#006648`, active `#005239`, disabled text `#adadad` on `#f5f5f5`.
  - Secondary: `#fff` bg, `#00855d` text, `box-shadow:inset 0 0 0 1px #00855d`. Hover fills `#00855d` with white text, active `#005239`. Disabled is `#adadad` text on transparent with `inset 0 0 0 1px #ccc`.

### Not found

- The "Design Help" labelled button with its circular-arrow-and-sparkle glyph (see section 2).
- The en_US text of `COMMON_UNTITLED_PROJECT_CANVAS_IQ`.
- The Braze card content for the trial pill (remote).


## 9. Operation rendering on the canvas

Design Space has two layer renderers. New and saved projects open through the CriCanvas flow by default: `doOpenNewCanvas` always calls `doOpenNewCriCanvas` (main.js:259509-259548), and `forceNonCriCanvasFlow: !0` (main.js:388785) is the only route back to the legacy flow. In the CriCanvas flow the operation is a typed Swift/WASM value. The canvas worker builds `LayerOperation.Cut(LayerOperationCut.Basic|Wavy|Perforate)`, `LayerOperation.Draw(Pen|Foil|Score|Deboss|Engrave)`, `Ptc`, `Guide` or `PrintOnly` (`design-space-app/canvas-webw-cricanvas.js@337757`). `CriCanvas.wasm` then emits finished renderables, each with its own `stroke.color`, `stroke.width`, `stroke.dashPattern` and `stroke.staticWidth`, and JS only rasterises them (`canvas-webw-cricanvas.js@303157`, `canvas-webw-render.js@329346`). The WASM was not decompiled. What JS does hold is the layer model (`layerStroke`, `layerFill`): `updateGroupsLineTypeBasic` writes per-operation defaults into it (chunk-JVMBAEXI.js:179823-179896), and the legacy JS renderer draws those values directly. The table below is that stored style, so read it as Design Space's visual language rather than proven CriCanvas pixels.

Units. On the legacy path `lineWidth = strokeWidth * scale`, where `scale` is the view scale for DRAW and 1 for every other operation (chunk-JVMBAEXI.js:191572-191582, chunk-JAGVFCCQ.js:6758-6768). Non-Pen strokes and their dashes are therefore constant in screen pixels, and only Pen strokes follow the zoom. On the CriCanvas path a stroke flagged `staticWidth` scales with zoom and any other stays in device pixels (chunk-JVMBAEXI.js:200057-200059, 200309-200310). Which operations set `staticWidth`: no evidence (decided inside the WASM).

| Operation | lineType | Design Space canvas style | Selected | Evidence |
|---|---|---|---|---|
| Basic Cut | `CUT` | Solid fill in the layer colour; a new cut layer with no previous colour gets `DEFAULT_CUT_COLOR` `#494949`. Stored stroke: centre, width 1 (`DEFAULT_CUT_STROKE_WIDTH`), `#000101`, no dash. | Shared bounding box only | main.js:82417, 82423-82424; chunk-JVMBAEXI.js:179985-180006 |
| Wavy Cut | `WAVE` | `fillType SOLID` in the layer colour, centre stroke width 1 (`DEFAULT_WAVE_WIDTH`), `#000000`, no dash. JS draws the plain outline; no evidence of a drawn wave (if CriCanvas draws one, it is inside the WASM). | Bounding box only | main.js:82419, 82435; chunk-JVMBAEXI.js:179877-179883 |
| Perforate | `PERF_BASIC` | `fillType NONE`, centre stroke width 1, dash `[15, 5]` (`DEFAULT_PERF_BASIC_DASH`), `#000000`. The WASM type takes no colour. | Bounding box only | main.js:82431-82432; chunk-JVMBAEXI.js:179867-179875 |
| Pen | `DRAW` | `fillType NONE`, centre stroke, no dash, in the pen colour (default `#000101`). Width is `nibSize_mm * 0.0393701 * 72` points (`NIB_SIZE_TO_STROKE_WIDTH_RATIO`) and scales with zoom. | Bounding box only | main.js:82417, 82420, 82422; chunk-JVMBAEXI.js:179832-179837, 180119-180145, 191578-191582 |
| Foil (Fine, Medium, Bold) | `MIDAS_FINE`, `MIDAS_REGULAR`, `MIDAS_BOLD` | `fillType NONE`, centre stroke, no dash, width 1, 2 and 3. Converting from a non-foil type sets the colour to `DEFAULT_MIDAS_COLOR` `#a48e42`. No foil texture found; the `midas-tool-*.png` files are tool photos. | Bounding box only | main.js:82591; chunk-JVMBAEXI.js:180018-180031 (`updateLayersMidas`) |
| Score | `SCORE` | `fillType NONE`, centre stroke width 1 (`DEFAULT_SCORE_WIDTH`), dash `[20, 20]` (`DEFAULT_SCORE_DASH`), `#000000`, inverted against a dark background by `invertScoreColorIfNeed`. Layer thumbnails use width 0.5. | Bounding box only; hit area 15 (`SCORELINE_HIT_AREA`) | main.js:82419, 82426-82427, 82445, 82454; chunk-JVMBAEXI.js:179839-179846 |
| Deboss | `DEBOSS_FINE` | `fillType NONE`, centre stroke width 3 (`DEFAULT_DEBOSS_FINE_WIDTH`), no dash, `#000000`. No pattern or texture found. | Bounding box only | main.js:82429; chunk-JVMBAEXI.js:179858-179866 |
| Engrave | `ENGRAVE` (also `_FILL`, `_OUTLINE`, `_PHOTO`) | `fillType NONE`, centre stroke width 1 (`DEFAULT_ENGRAVE_WIDTH`), no dash, `#000000`. No hatch or pattern found. | Bounding box only | main.js:82428; chunk-JVMBAEXI.js:179848-179857 |
| Print Then Cut | `PRINTCUT` | `fillType BITMAP`, falling back to `SOLID`; stroke type `NONE`, so no outline on the canvas. Print bleed constants are `BLEED_DISTANCE` 1 and `BLEED_DISTANCE_NON_PREVIEW` 9, used in print image generation, not on the canvas. | Bounding box only | chunk-JVMBAEXI.js:179829-179831, 179937-179954, 180009-180017; main.js:82439-82440, 367160 |
| Guide | `GUIDE` (also `_FILL`, `_OUTLINE`) | `fillType NONE`, centre stroke width 1, no dash, `LAYER_GUIDE_COLOR` `#D6008B` (magenta), brightened when it is close to the background. | Bounding box only; a guide in a group makes a "guide" group | main.js:82582; chunk-JVMBAEXI.js:179885-179891, 227314-227393 |

Notes:
- Hover. CriCanvas fills every path of a group whose uuid contains `hover` with `rgba(0, 0, 0, 0.1)` (chunk-JVMBAEXI.js:200453-200461). There is no per-operation selected or hover style: selection is the shared bounding box (`renderBoundingBoxToContext`, chunk-JVMBAEXI.js:200370; constants main.js:82540-82556).
- Mat preview. Cut and Wavy layers draw in the material colour. The legacy mat canvas strokes black when fill luminance `sqrt(0.299R² + 0.587G² + 0.114B²)` exceeds 127.5 and white otherwise (chunk-JVMBAEXI.js:191566-191570, 191625-191645); CriCanvas `matCanvas` switches the stroke only when the fill equals the mat colour (`canvas-webw-cricanvas.js@302022`, `@305140`). Guide and print-only layers are left off mats (chunk-JVMBAEXI.js:224444, 229490-229493).
- Unused constants `LAYER_SCORE_DASH [20,20]`, `LAYER_PERF_BASIC_DASH [15,5]`, `LAYER_DEBOSS_WIDTH 3` and `LAYER_PERF_STITCH_DASH [15,15]` (main.js:82442-82453) have no JS readers and may mirror values baked into the WASM.
- Layer panel. The operation dropdown trigger uses four glyph groups, `cut`, `draw`, `print` and `guides` (main.js:161064-161074). Layer rows show rendered thumbnails, not operation icons.
- Reference image. Nothing in the asar shows operations on the canvas; the nearest are `static-asset/design3/perforated-cut.svg` and the `illustrations/midas-tool-*.png` tool photos. The evidence above stands in for a reference screenshot.

### What ODS draws (`src/ui/op_style.rs`)

ODS copies the table where it is unambiguous and improves on it where Design Space gives no cue.

| Operation | ODS | Versus Design Space |
|---|---|---|
| Basic Cut | Fill in the layer colour. | Same. |
| Wavy Cut | Fill, edged by a 2 pt sine wave every 8 pt in a darker tone. | Design Space draws a plain 1px outline; ODS shows the wave. |
| Perforate | No fill, 1 pt dashes 15 on 5 off, ink. | Same dash and width. |
| Pen | No fill, stroke at the nib width in the pen colour, following zoom. | Same rule. |
| Foil | No fill, 1, 2 or 3 pt stroke in the layer colour with a diagonal light and dark sheen. | Same widths; ODS adds the sheen so foil never reads as pen. |
| Score | No fill, 1 pt dashes 20 on 20, ink. | Same. |
| Deboss | No fill, 3 pt ink stroke with a pale centre line, so it reads as a groove. | Same width; ODS adds the groove. |
| Engrave | No fill, 1 pt solid ink. | Same. |
| Print Then Cut | The printed fill, a 1 pt cut line in a darker tone and a faint drop shadow. | Design Space draws no outline; ODS shows where the blade goes. |
| Guide | No fill, 1 pt `TEXT_MUTED` dashes 4 on 4 at 60 % opacity, skipped by marquee selection. | Design Space draws solid magenta; ODS dashes it in its own muted ink so it never passes for a cut. |

Dashes, waves and every non-Pen width are screen points, as in Design Space. Show as made drops fills from cuts (outline only), hides guides and drops the Print Then Cut shadow. Mat previews use the same table, and a fill matching the mat colour gets a black or white outline by Design Space's luminance rule.
