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
            let scaled = image
                .thumbnail(cell_w as u32, (cell_h as u32) * 2)
                .to_rgb8();
            let picture_w = scaled.width() as usize;
            let picture_h = scaled.height() as usize;
            let raw = scaled.as_raw();
            let picture = half_blocks(picture_w, picture_h, raw);
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

fn color_at(rgb: &[u8], width: usize, x: usize, y: usize) -> u16 {
    let i = (y * width + x) * 3;
    cube(rgb[i], rgb[i + 1], rgb[i + 2])
}

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
}
