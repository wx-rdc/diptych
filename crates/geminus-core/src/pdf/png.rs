//! Flate 位图 → PNG 重编码（PDF 嵌入图第二种主流编码）。
//!
//! `pdf` crate 的 `raw_image_data` 对 FlateDecode 返回解码后的原始采样，
//! 浏览器无法直接渲染；此处按 ColorSpace/BitsPerComponent 组装 PNG。

/// 8 位采样 → PNG 字节流。color_type: 0=灰度 2=RGB
pub fn encode_png(samples: &[u8], width: u32, height: u32, color_type: u8) -> Vec<u8> {
    let bpp = if color_type == 0 { 1 } else { 3 };
    let stride = width as usize * bpp;
    let mut raw = Vec::with_capacity((stride + 1) * height as usize);
    for row in 0..height as usize {
        raw.push(0u8); // filter: None
        let s = row * stride;
        raw.extend_from_slice(&samples[s..s + stride]);
    }
    let mut out = Vec::with_capacity(raw.len() / 2 + 128);
    out.extend_from_slice(b"\x89PNG\r\n\x1a\n");
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, color_type, 0, 0, 0]); // 8bit/ct/deflate/adaptive/no filter
    chunk(&mut out, b"IHDR", &ihdr);
    chunk(&mut out, b"IDAT", &zlib_compress(&raw));
    chunk(&mut out, b"IEND", &[]);
    out
}

/// CMYK → RGB（朴素转换，无 ICC）
pub fn cmyk_to_rgb(samples: &[u8]) -> Vec<u8> {
    samples
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|c| {
            let (c, m, y, k) = (c[0] as f32 / 255.0, c[1] as f32 / 255.0, c[2] as f32 / 255.0, c[3] as f32 / 255.0);
            let f = |v: f32| ((1.0 - v) * (1.0 - k) * 255.0).round().clamp(0.0, 255.0) as u8;
            [f(c), f(m), f(y)]
        })
        .collect()
}

/// 1-bit 灰度（含 ImageMask 语义取反）→ 8-bit
pub fn expand_gray1(samples: &[u8], npix: usize, invert: bool) -> Vec<u8> {
    let mut out = Vec::with_capacity(npix);
    for (i, b) in samples.iter().take(npix.div_ceil(8)).enumerate() {
        for bit in (0..8).rev() {
            if out.len() >= npix {
                break;
            }
            let v = (b >> bit) & 1;
            let v = if invert { 1 - v } else { v };
            out.push(if v == 1 { 255 } else { 0 });
        }
        let _ = i;
    }
    out
}

fn chunk(out: &mut Vec<u8>, typ: &[u8; 4], data: &[u8]) {
    out.extend_from_slice(&(data.len() as u32).to_be_bytes());
    let mut crc_input = Vec::with_capacity(4 + data.len());
    crc_input.extend_from_slice(typ);
    crc_input.extend_from_slice(data);
    out.extend_from_slice(typ);
    out.extend_from_slice(data);
    out.extend_from_slice(&crc32(&crc_input).to_be_bytes());
}

fn zlib_compress(data: &[u8]) -> Vec<u8> {
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write as _;
    let mut enc = ZlibEncoder::new(Vec::new(), Compression::fast());
    enc.write_all(data).expect("zlib 内存写入不会失败");
    enc.finish().expect("zlib finish 不会失败")
}

fn crc32(data: &[u8]) -> u32 {
    // PNG 规范 CRC-32（反射多项式 0xEDB88320），查表法
    let mut table = [0u32; 256];
    for (n, t) in table.iter_mut().enumerate() {
        let mut c = n as u32;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
        }
        *t = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_signature_and_structure() {
        let png = encode_png(&[10, 20, 30, 40, 50, 60], 2, 1, 2);
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
        assert_eq!(&png[12..16], b"IHDR");
        assert_eq!(&png[16..20], &2u32.to_be_bytes());
        assert_eq!(&png[20..24], &1u32.to_be_bytes());
        assert!(png.windows(4).any(|w| w == b"IDAT"));
        assert!(png.windows(4).any(|w| w == b"IEND"));
        // PNG 可被标准库校验：crc 尾部长度正确（总长 = 8 + 25 + (13+12+len+...)）
        let iend_pos = png.windows(4).rposition(|w| w == b"IEND").unwrap();
        assert_eq!(png.len(), iend_pos + 4 + 4);
    }

    #[test]
    fn cmyk_conversion() {
        let rgb = cmyk_to_rgb(&[0, 0, 0, 0, 255, 255, 255, 0]);
        assert_eq!(rgb, vec![255, 255, 255, 0, 0, 0]); // 白 / 黑（减色）
    }

    #[test]
    fn gray1_expansion() {
        assert_eq!(expand_gray1(&[0b1010_0000], 4, false), vec![255, 0, 255, 0]);
        assert_eq!(expand_gray1(&[0b1000_0000], 1, true), vec![0]);
    }

    #[test]
    fn crc32_known_vector() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }
}
