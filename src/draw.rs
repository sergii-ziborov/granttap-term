use std::path::Path;

use crate::model::ImageMark;

pub struct Preview {
    pub mark: ImageMark,
    pub picture: String,
    pub sixel: String,
    pub kitty: String,
    pub iterm: String,
}

pub fn inspect(path: &Path, n: usize, cols: usize, rows: usize) -> Result<Preview, String> {
    let meta = std::fs::metadata(path).map_err(|err| err.to_string())?;
    let format = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("image")
        .to_ascii_lowercase();
    let (width, height, picture, sixel, kitty, iterm) = match image::open(path) {
        Ok(image) => {
            let width = image.width();
            let height = image.height();
            let cell_w = cols.clamp(8, 48);
            let cell_h = rows.clamp(4, 18);
            let picture = portrait_of(&image, cell_w, cell_h);
            let scaled = image
                .thumbnail(cell_w as u32, (cell_h as u32) * 2)
                .to_rgb8();
            let picture_w = scaled.width() as usize;
            let picture_h = scaled.height() as usize;
            let raw = scaled.as_raw();
            let sixel = crate::sixel::encode(picture_w, picture_h, raw);
            let kitty = crate::inline::kitty(picture_w, picture_h, raw);
            let iterm = crate::inline::iterm(picture_w, picture_h, raw);
            (width, height, picture, sixel, kitty, iterm)
        }
        Err(err) => return Err(err.to_string()),
    };
    Ok(Preview {
        mark: ImageMark {
            n,
            format,
            width,
            height,
            bytes: meta.len(),
            path: path.to_path_buf(),
        },
        picture,
        sixel,
        kitty,
        iterm,
    })
}

/// Fit the file into a cell grid. Each cell is four source pixels, so a
/// screenshot keeps twice the horizontal detail of a half-block portrait.
pub fn portrait(path: &Path, max_cols: usize, max_rows: usize) -> Result<String, String> {
    let image = image::open(path).map_err(|err| err.to_string())?;
    Ok(portrait_of(&image, max_cols, max_rows))
}

/// Draw into the cell budget. Each cell is six samples (two by three), and the
/// sample grid is corrected for an 8×16 console cell so a square photo stays square.
pub(crate) fn portrait_of(image: &image::DynamicImage, max_cols: usize, max_rows: usize) -> String {
    let src_w = image.width().max(1);
    let src_h = image.height().max(1);
    let max_cols = max_cols.clamp(1, 240);
    let max_rows = max_rows.clamp(1, 72);
    let max_px_w = (max_cols * 2) as u32;
    let max_px_h = (max_rows * 3) as u32;
    let aspect = (f64::from(src_w) / f64::from(src_h)) * (16.0 / 8.0) * (2.0 / 3.0);
    let mut width = max_px_w;
    let mut height = max_px_h;
    let box_aspect = f64::from(max_px_w) / f64::from(max_px_h);
    if aspect > box_aspect {
        height = ((f64::from(max_px_w) / aspect).round() as u32).max(3);
    } else {
        width = ((f64::from(max_px_h) * aspect).round() as u32).max(2);
    }
    let width = (width / 2 * 2).clamp(2, max_px_w);
    let height = (height / 3 * 3).clamp(3, max_px_h);
    let scaled = image
        .resize_exact(width, height, image::imageops::FilterType::Lanczos3)
        .to_rgb8();
    sextant_blocks(
        scaled.width() as usize,
        scaled.height() as usize,
        scaled.as_raw(),
    )
}

/// Cell size for a logo on the text page. A 272×92 wordmark stays wide and
/// short. A square mark stays within four rows, so the page text stays on screen.
pub(crate) fn header_box(px_w: u32, px_h: u32) -> (usize, usize) {
    let cols = (px_w.max(1) as usize + 7) / 8;
    let rows = (px_h.max(1) as usize + 15) / 16;
    fit_cells(cols.max(1), rows.max(1), 36, 4)
}

/// Shrink a cell box that is larger than the cap. A box that already fits is unchanged.
pub(crate) fn fit_cells(cols: usize, rows: usize, max_cols: usize, max_rows: usize) -> (usize, usize) {
    let cols = cols.max(1);
    let rows = rows.max(1);
    let max_cols = max_cols.max(1);
    let max_rows = max_rows.max(1);
    if cols <= max_cols && rows <= max_rows {
        return (cols, rows);
    }
    let scale = (max_cols as f64 / cols as f64).min(max_rows as f64 / rows as f64);
    let wide = ((cols as f64) * scale).round().max(1.0) as usize;
    let high = ((rows as f64) * scale).round().max(1.0) as usize;
    (wide.min(max_cols).max(1), high.min(max_rows).max(1))
}

/// Stretch the bitmap into the element's cell box. The box is already the
/// CSS rectangle in 8×16 cells, capped so one picture cannot fill the pane.
pub(crate) fn portrait_fill(image: &image::DynamicImage, cols: usize, rows: usize) -> String {
    let cols = cols.clamp(1, 64);
    let rows = rows.clamp(1, 12);
    let scaled = image
        .resize_exact(
            (cols * 2) as u32,
            (rows * 3) as u32,
            image::imageops::FilterType::Lanczos3,
        )
        .to_rgb8();
    sextant_blocks(scaled.width() as usize, scaled.height() as usize, scaled.as_raw())
}

pub(crate) fn has_blocks(text: &str) -> bool {
    text.chars().any(|ch| {
        ('\u{2580}'..='\u{259F}').contains(&ch) || ('\u{1FB00}'..='\u{1FB3B}').contains(&ch)
    })
}

#[cfg(test)]
pub fn half_blocks(width: usize, height: usize, rgb: &[u8]) -> String {
    if width == 0 || height == 0 || rgb.len() < width * height * 3 {
        return String::new();
    }
    let mut out = String::new();
    let mut y = 0;
    while y < height {
        let mut last = String::new();
        for x in 0..width {
            let top = color_at(rgb, width, x, y);
            let bottom = if y + 1 < height {
                color_at(rgb, width, x, y + 1)
            } else {
                0
            };
            let ink = format!("\x1b[38;5;{top};48;5;{bottom}m");
            if ink != last {
                out.push_str(&ink);
                last = ink;
            }
            out.push('▀');
        }
        out.push_str("\x1b[0;48;5;0m\n");
        y += 2;
    }
    out
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn half_blocks_true(width: usize, height: usize, rgb: &[u8]) -> String {
    if width == 0 || height == 0 || rgb.len() < width * height * 3 {
        return String::new();
    }
    let mut out = String::new();
    let mut y = 0;
    while y < height {
        let mut primed = false;
        let mut last_top = [0u8; 3];
        let mut last_bot = [0u8; 3];
        for x in 0..width {
            let top = pixel(rgb, width, x, y);
            let bot = if y + 1 < height {
                pixel(rgb, width, x, y + 1)
            } else {
                [0, 0, 0]
            };
            if !primed || top != last_top || bot != last_bot {
                out.push_str(&format!(
                    "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                    top[0], top[1], top[2], bot[0], bot[1], bot[2]
                ));
                last_top = top;
                last_bot = bot;
                primed = true;
            }
            out.push('▀');
        }
        out.push_str("\x1b[0;48;5;0m\n");
        y += 2;
    }
    out
}

/// Bit 0 is the top left, then top right, bottom left, bottom right.
#[cfg(test)]
const QUAD: [char; 16] = [
    ' ', '▘', '▝', '▀', '▖', '▌', '▞', '▛', '▗', '▚', '▐', '▜', '▄', '▙', '▟', '█',
];

#[cfg(test)]
fn quadrant_blocks(width: usize, height: usize, rgb: &[u8]) -> String {
    if width < 2 || height < 2 || rgb.len() < width * height * 3 {
        return String::new();
    }
    let mut out = String::new();
    let mut y = 0;
    while y + 1 < height {
        let mut primed = false;
        let mut last_fg = [0u8; 3];
        let mut last_bg = [0u8; 3];
        let mut x = 0;
        while x + 1 < width {
            let cell = [
                pixel(rgb, width, x, y),
                pixel(rgb, width, x + 1, y),
                pixel(rgb, width, x, y + 1),
                pixel(rgb, width, x + 1, y + 1),
            ];
            let (glyph, fg, bg) = quadrant_cell(cell);
            if !primed || fg != last_fg || bg != last_bg {
                out.push_str(&format!(
                    "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                    fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]
                ));
                last_fg = fg;
                last_bg = bg;
                primed = true;
            }
            out.push(glyph);
            x += 2;
        }
        out.push_str("\x1b[0;48;5;0m\n");
        y += 2;
    }
    out
}

/// Bit 0 is the top left, then top right, middle left, middle right, bottom left, bottom right.
fn sextant_blocks(width: usize, height: usize, rgb: &[u8]) -> String {
    if width < 2 || height < 3 || rgb.len() < width * height * 3 {
        return String::new();
    }
    let mut out = String::new();
    let mut y = 0;
    while y + 2 < height {
        let mut primed = false;
        let mut last_fg = [0u8; 3];
        let mut last_bg = [0u8; 3];
        let mut x = 0;
        while x + 1 < width {
            let cell = [
                pixel(rgb, width, x, y),
                pixel(rgb, width, x + 1, y),
                pixel(rgb, width, x, y + 1),
                pixel(rgb, width, x + 1, y + 1),
                pixel(rgb, width, x, y + 2),
                pixel(rgb, width, x + 1, y + 2),
            ];
            let (glyph, fg, bg) = sextant_cell(cell);
            if !primed || fg != last_fg || bg != last_bg {
                out.push_str(&format!(
                    "\x1b[38;2;{};{};{};48;2;{};{};{}m",
                    fg[0], fg[1], fg[2], bg[0], bg[1], bg[2]
                ));
                last_fg = fg;
                last_bg = bg;
                primed = true;
            }
            out.push(glyph);
            x += 2;
        }
        out.push_str("\x1b[0;48;5;0m\n");
        y += 3;
    }
    out
}

fn sextant_cell(px: [[u8; 3]; 6]) -> (char, [u8; 3], [u8; 3]) {
    let mut best = 0u32;
    let mut left = 0usize;
    let mut right = 1usize;
    for i in 0..6 {
        for j in (i + 1)..6 {
            let distance = dist2(px[i], px[j]);
            if distance > best {
                best = distance;
                left = i;
                right = j;
            }
        }
    }
    if best < 10 * 10 {
        let flat = average6(&px);
        return ('█', flat, flat);
    }
    let mut mask = 0u8;
    let mut sum_fg = [0u16; 3];
    let mut sum_bg = [0u16; 3];
    let mut n_fg = 0u16;
    let mut n_bg = 0u16;
    for (index, pixel) in px.iter().enumerate() {
        if dist2(*pixel, px[left]) <= dist2(*pixel, px[right]) {
            mask |= 1 << index;
            add(&mut sum_fg, *pixel);
            n_fg += 1;
        } else {
            add(&mut sum_bg, *pixel);
            n_bg += 1;
        }
    }
    if n_fg == 0 || n_bg == 0 {
        let flat = average6(&px);
        return ('█', flat, flat);
    }
    (sextant_glyph(mask), div(sum_fg, n_fg), div(sum_bg, n_bg))
}

fn sextant_glyph(mask: u8) -> char {
    match mask {
        0x00 => ' ',
        0x15 => '▌',
        0x2a => '▐',
        0x3f => '█',
        i if i >= 0x2a => char::from_u32(0x1FB00 | (u32::from(i) - 3)).unwrap_or('█'),
        i if i >= 0x15 => char::from_u32(0x1FB00 | (u32::from(i) - 2)).unwrap_or('█'),
        i => char::from_u32(0x1FB00 | (u32::from(i) - 1)).unwrap_or('█'),
    }
}

fn average6(px: &[[u8; 3]; 6]) -> [u8; 3] {
    let mut sum = [0u16; 3];
    for pixel in px {
        add(&mut sum, *pixel);
    }
    div(sum, 6)
}

#[cfg(test)]
fn quadrant_cell(px: [[u8; 3]; 4]) -> (char, [u8; 3], [u8; 3]) {
    let mut best = 0u32;
    let mut left = 0usize;
    let mut right = 1usize;
    for i in 0..4 {
        for j in (i + 1)..4 {
            let distance = dist2(px[i], px[j]);
            if distance > best {
                best = distance;
                left = i;
                right = j;
            }
        }
    }
    if best < 24 * 24 {
        let flat = average(&px);
        return ('█', flat, flat);
    }
    let mut mask = 0u8;
    let mut sum_fg = [0u16; 3];
    let mut sum_bg = [0u16; 3];
    let mut n_fg = 0u16;
    let mut n_bg = 0u16;
    for (index, pixel) in px.iter().enumerate() {
        if dist2(*pixel, px[left]) <= dist2(*pixel, px[right]) {
            mask |= 1 << index;
            add(&mut sum_fg, *pixel);
            n_fg += 1;
        } else {
            add(&mut sum_bg, *pixel);
            n_bg += 1;
        }
    }
    if n_fg == 0 || n_bg == 0 {
        let flat = average(&px);
        return ('█', flat, flat);
    }
    (QUAD[mask as usize], div(sum_fg, n_fg), div(sum_bg, n_bg))
}

fn dist2(a: [u8; 3], b: [u8; 3]) -> u32 {
    let dr = i32::from(a[0]) - i32::from(b[0]);
    let dg = i32::from(a[1]) - i32::from(b[1]);
    let db = i32::from(a[2]) - i32::from(b[2]);
    (dr * dr + dg * dg + db * db) as u32
}

fn add(sum: &mut [u16; 3], pixel: [u8; 3]) {
    sum[0] += u16::from(pixel[0]);
    sum[1] += u16::from(pixel[1]);
    sum[2] += u16::from(pixel[2]);
}

fn div(sum: [u16; 3], n: u16) -> [u8; 3] {
    [
        (sum[0] / n) as u8,
        (sum[1] / n) as u8,
        (sum[2] / n) as u8,
    ]
}

#[cfg(test)]
fn average(px: &[[u8; 3]; 4]) -> [u8; 3] {
    let mut sum = [0u16; 3];
    for pixel in px {
        add(&mut sum, *pixel);
    }
    div(sum, 4)
}

fn pixel(rgb: &[u8], width: usize, x: usize, y: usize) -> [u8; 3] {
    let i = (y * width + x) * 3;
    [rgb[i], rgb[i + 1], rgb[i + 2]]
}

#[cfg(test)]
fn color_at(rgb: &[u8], width: usize, x: usize, y: usize) -> u16 {
    let i = (y * width + x) * 3;
    cube(rgb[i], rgb[i + 1], rgb[i + 2])
}

#[cfg(test)]
fn cube(r: u8, g: u8, b: u8) -> u16 {
    let ri = u16::from(r) * 5 / 255;
    let gi = u16::from(g) * 5 / 255;
    let bi = u16::from(b) * 5 / 255;
    16 + 36 * ri + 6 * gi + bi
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn half_blocks_pair_two_rows_into_one() {
        let rgb = [
            255, 0, 0, 0, 0, 255, // row 0
            0, 255, 0, 0, 0, 0, // row 1
        ];
        let text = half_blocks(2, 2, &rgb);
        assert!(text.contains('▀'));
        assert!(text.contains("38;5;"));
        assert!(text.contains("48;5;"));
        assert_eq!(text.chars().filter(|ch| *ch == '▀').count(), 2);
    }

    #[test]
    fn short_buffer_draws_nothing() {
        assert!(half_blocks(2, 2, &[0, 0, 0]).is_empty());
    }

    #[test]
    fn truecolor_half_blocks_keep_the_pixel() {
        let rgb = [
            255, 0, 0, 0, 0, 255, // row 0
            0, 255, 0, 0, 0, 0, // row 1
        ];
        let text = half_blocks_true(2, 2, &rgb);
        assert!(text.contains('▀'));
        assert!(text.contains("38;2;255;0;0"), "{text}");
        assert!(text.contains("48;2;0;255;0"), "{text}");
        assert_eq!(text.chars().filter(|ch| *ch == '▀').count(), 2);
    }

    #[test]
    fn quadrants_keep_a_vertical_edge() {
        let rgb = [
            255, 0, 0, 0, 0, 255, // row 0
            255, 0, 0, 0, 0, 255, // row 1
        ];
        let text = quadrant_blocks(2, 2, &rgb);
        assert!(text.contains('▌'), "{text}");
        assert!(text.contains("38;2;255;0;0"), "{text}");
        assert!(text.contains("48;2;0;0;255"), "{text}");
        assert!(has_blocks(&text));
    }

    #[test]
    fn sextants_keep_a_vertical_edge() {
        let mut rgb = vec![0u8; 2 * 3 * 3];
        for y in 0..3 {
            rgb[y * 6..y * 6 + 3].copy_from_slice(&[255, 0, 0]);
            rgb[y * 6 + 3..y * 6 + 6].copy_from_slice(&[0, 0, 255]);
        }
        let text = sextant_blocks(2, 3, &rgb);
        assert!(text.contains('▌'), "{text}");
        assert!(text.contains("38;2;255;0;0"), "{text}");
        assert!(text.contains("48;2;0;0;255"), "{text}");
        assert!(has_blocks(&text));
    }

    #[test]
    fn a_wordmark_stays_inside_a_header() {
        let (cols, rows) = header_box(272, 92);
        assert!(cols >= 16 && cols <= 36, "{cols} {rows}");
        assert!(rows <= 4, "{cols} {rows}");
        let (cols, rows) = header_box(400, 400);
        assert!(cols <= 36 && rows <= 4, "{cols} {rows}");
        assert_eq!(header_box(40, 20), (5, 2));
        let (wide, high) = fit_cells(160, 48, 64, 12);
        assert!(wide <= 64 && high <= 12 && high < 48, "{wide} {high}");
        let mut raw = vec![20u8; 272 * 92 * 3];
        for pixel in raw.chunks_mut(3) {
            pixel.copy_from_slice(&[20, 80, 200]);
        }
        let image = image::DynamicImage::ImageRgb8(image::RgbImage::from_raw(272, 92, raw).unwrap());
        let (cols, rows) = header_box(image.width(), image.height());
        let picture = portrait_of(&image, cols, rows);
        let n = picture.lines().filter(|line| !line.is_empty()).count();
        assert!(n <= 4, "{n}");
        assert!(has_blocks(&picture));
    }

    #[test]
    fn sextant_top_left_is_the_first_mosaic() {
        let mut rgb = vec![0u8; 2 * 3 * 3];
        rgb[0] = 255;
        let text = sextant_blocks(2, 3, &rgb);
        assert!(text.contains('\u{1FB00}'), "{text}");
        assert!(has_blocks(&text));
    }
}
