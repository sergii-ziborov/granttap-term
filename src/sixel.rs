use std::collections::HashMap;
use std::fmt::Write;

pub fn encode(width: usize, height: usize, rgb: &[u8]) -> String {
    if width == 0 || height == 0 {
        return String::new();
    }
    // The product has to fit; otherwise this slice cannot hold the image.
    let Some(need) = width.checked_mul(height).and_then(|n| n.checked_mul(3)) else {
        return String::new();
    };
    if rgb.len() < need {
        return String::new();
    }

    let (palette, indices) = quantize(width, height, rgb);
    let mut out = String::new();
    out.push_str("\x1bPq");
    write!(out, "\"1;1;{width};{height}").unwrap();
    for (i, color) in palette.iter().enumerate() {
        write!(
            out,
            "#{i};2;{};{};{}",
            pct(color[0]),
            pct(color[1]),
            pct(color[2])
        )
        .unwrap();
    }
    write_bands(&mut out, width, height, &indices);
    out.push_str("\x1b\\");
    out
}

fn pct(channel: u8) -> u8 {
    (u16::from(channel) * 100 / 255) as u8
}

fn pixel(rgb: &[u8], i: usize) -> [u8; 3] {
    let p = i * 3;
    [rgb[p], rgb[p + 1], rgb[p + 2]]
}

fn quantize(width: usize, height: usize, rgb: &[u8]) -> (Vec<[u8; 3]>, Vec<u8>) {
    let pixels = width * height;
    let mut seen: HashMap<[u8; 3], usize> = HashMap::new();
    let mut order = Vec::new();
    let mut counts = Vec::new();
    for i in 0..pixels {
        let color = pixel(rgb, i);
        match seen.get(&color).copied() {
            Some(idx) => counts[idx] += 1,
            None => {
                seen.insert(color, order.len());
                order.push(color);
                counts.push(1);
            }
        }
    }

    if order.len() <= 16 {
        let mut indices = Vec::with_capacity(pixels);
        for i in 0..pixels {
            indices.push(seen[&pixel(rgb, i)] as u8);
        }
        return (order, indices);
    }

    let palette = most_common(&order, &counts, 16);
    let mut indices = Vec::with_capacity(pixels);
    for i in 0..pixels {
        indices.push(nearest(&palette, pixel(rgb, i)));
    }
    (palette, indices)
}

fn most_common(order: &[[u8; 3]], counts: &[usize], limit: usize) -> Vec<[u8; 3]> {
    let mut ranked: Vec<usize> = (0..order.len()).collect();
    ranked.sort_by(|&a, &b| counts[b].cmp(&counts[a]).then(a.cmp(&b)));
    ranked.truncate(limit);
    ranked.into_iter().map(|idx| order[idx]).collect()
}

fn nearest(palette: &[[u8; 3]], color: [u8; 3]) -> u8 {
    let mut best = 0u8;
    let mut best_d = u32::MAX;
    for (i, sample) in palette.iter().enumerate() {
        let dr = u32::from(color[0].abs_diff(sample[0]));
        let dg = u32::from(color[1].abs_diff(sample[1]));
        let db = u32::from(color[2].abs_diff(sample[2]));
        let d = dr * dr + dg * dg + db * db;
        if d < best_d {
            best_d = d;
            best = i as u8;
            if d == 0 {
                break;
            }
        }
    }
    best
}

fn write_bands(out: &mut String, width: usize, height: usize, indices: &[u8]) {
    let mut y = 0;
    while y < height {
        let rows = (height - y).min(6);
        let mut used = [false; 16];
        for row in 0..rows {
            let start = (y + row) * width;
            for &idx in &indices[start..start + width] {
                used[idx as usize] = true;
            }
        }
        let colors: Vec<u8> = (0..used.len())
            .filter(|&c| used[c])
            .map(|c| c as u8)
            .collect();
        let last_band = y + 6 >= height;
        for (n, color) in colors.iter().enumerate() {
            write!(out, "#{color}").unwrap();
            let mut x = 0;
            while x < width {
                let bits = column_bits(indices, width, x, y, rows, *color);
                let mut run = 1;
                while x + run < width
                    && column_bits(indices, width, x + run, y, rows, *color) == bits
                {
                    run += 1;
                }
                push_sixel(out, bits, run);
                x += run;
            }
            if n + 1 != colors.len() {
                out.push('$');
            } else if !last_band {
                out.push('-');
            }
        }
        y += 6;
    }
}

fn column_bits(indices: &[u8], width: usize, x: usize, y: usize, rows: usize, color: u8) -> u8 {
    let mut bits = 0u8;
    for row in 0..rows {
        if indices[(y + row) * width + x] == color {
            // Bit 0 is the top pixel of the six-pixel column.
            bits |= 1 << row;
        }
    }
    bits
}

fn push_sixel(out: &mut String, bits: u8, run: usize) {
    let ch = char::from(bits + 63);
    if run >= 4 {
        write!(out, "!{run}").unwrap();
        out.push(ch);
    } else {
        for _ in 0..run {
            out.push(ch);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn two_by_two_starts_with_sixel_introducer() {
        let rgb = [
            255, 0, 0, 0, 0, 255, // row 0
            0, 255, 0, 255, 255, 255, // row 1
        ];
        let text = encode(2, 2, &rgb);
        assert!(!text.is_empty());
        assert!(text.starts_with("\x1bPq"));
        assert!(text.ends_with("\x1b\\"));
    }

    #[test]
    fn short_buffer_returns_empty() {
        assert!(encode(2, 2, &[0, 0, 0]).is_empty());
        assert!(encode(0, 1, &[0, 0, 0]).is_empty());
        assert!(encode(1, 0, &[0, 0, 0]).is_empty());
    }

    #[test]
    fn one_red_pixel_is_a_sixel_column() {
        assert_eq!(
            encode(1, 1, &[255, 0, 0]),
            "\x1bPq\"1;1;1;1#0;2;100;0;0#0@\x1b\\"
        );
    }

    #[test]
    fn at_most_sixteen_colors() {
        let mut rgb = Vec::new();
        for i in 0..32u8 {
            rgb.extend([i, i.wrapping_mul(3), 255 - i]);
        }
        let text = encode(32, 1, &rgb);
        assert!(text.starts_with("\x1bPq"));
        assert!(defined_colors(&text) <= 16);
        assert!(defined_colors(&text) >= 1);
    }

    fn defined_colors(text: &str) -> usize {
        let bytes = text.as_bytes();
        let mut count = 0;
        let mut i = 0;
        while i < bytes.len() {
            if bytes[i] == b'#' {
                let mut j = i + 1;
                let start = j;
                while j < bytes.len() && bytes[j].is_ascii_digit() {
                    j += 1;
                }
                if j > start && bytes.get(j..j + 3) == Some(b";2;") {
                    count += 1;
                    i = j + 3;
                    continue;
                }
            }
            i += 1;
        }
        count
    }
}
