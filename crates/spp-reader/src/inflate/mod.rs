//! DEFLATE decompression (RFC 1951), for the zlib-compressed parts of Painter's records (the mesh scene).
//! A copy of MaterialCraft's own decoder (`src/deflate`), kept here so the reader depends on no application crate.

mod tables;

use tables::{CODE_LENGTH_ORDER, DIST_BASE as DBASE, DIST_EXTRA as DEXT, LENGTH_BASE as LBASE, LENGTH_EXTRA as LEXT};

struct BitR<'a> {
    d: &'a [u8],
    pos: usize,
    bit: u32,
    cnt: u32,
}

impl<'a> BitR<'a> {
    fn bits(&mut self, n: u32) -> Result<u32, String> {
        while self.cnt < n {
            let b = *self
                .d
                .get(self.pos)
                .ok_or_else(|| "unexpected end of compressed data".to_string())?;
            self.bit |= (b as u32) << self.cnt;
            self.pos += 1;
            self.cnt += 8;
        }
        let v = self.bit & ((1u32 << n) - 1);
        self.bit >>= n;
        self.cnt -= n;
        Ok(v)
    }
}

struct Huff {
    count: [u16; 16],
    symbol: Vec<u16>,
}

fn build(lengths: &[u8]) -> Huff {
    let mut count = [0u16; 16];
    for &l in lengths {
        count[l as usize & 15] += 1;
    }
    let mut offs = [0u16; 16];
    for i in 1..15 {
        offs[i + 1] = offs[i] + count[i];
    }
    let mut symbol = vec![0u16; lengths.len()];
    for (s, &l) in lengths.iter().enumerate() {
        if l != 0 {
            symbol[offs[l as usize & 15] as usize] = s as u16;
            offs[l as usize & 15] += 1;
        }
    }
    Huff { count, symbol }
}

fn decode_sym(br: &mut BitR, h: &Huff) -> Result<u16, String> {
    let (mut code, mut first, mut index) = (0i32, 0i32, 0i32);
    for len in 1..16 {
        code |= br.bits(1)? as i32;
        let count = h.count[len] as i32;
        if code - count < first {
            return h
                .symbol
                .get((index + (code - first)) as usize)
                .copied()
                .ok_or_else(|| "bad huffman code".to_string());
        }
        index += count;
        first += count;
        first <<= 1;
        code <<= 1;
    }
    Err("bad huffman code".into())
}

fn codes(br: &mut BitR, out: &mut Vec<u8>, lh: &Huff, dh: &Huff) -> Result<(), String> {
    loop {
        let sym = decode_sym(br, lh)? as usize;
        if sym < 256 {
            out.push(sym as u8);
        } else if sym == 256 {
            return Ok(());
        } else {
            let s = sym - 257;
            if s >= 29 {
                return Err("bad length symbol".into());
            }
            let len = LBASE[s] as usize + br.bits(LEXT[s] as u32)? as usize;
            let ds = decode_sym(br, dh)? as usize;
            if ds >= 30 {
                return Err("bad distance symbol".into());
            }
            let dist = DBASE[ds] as usize + br.bits(DEXT[ds] as u32)? as usize;
            if dist > out.len() {
                return Err("distance too far back".into());
            }
            for _ in 0..len {
                let b = out[out.len() - dist];
                out.push(b);
            }
        }
        if out.len() > (1 << 30) {
            return Err("decompressed data too large".into());
        }
    }
}

pub fn inflate(data: &[u8]) -> Result<Vec<u8>, String> {
    let mut br = BitR {
        d: data,
        pos: 0,
        bit: 0,
        cnt: 0,
    };
    let mut out = Vec::new();
    loop {
        let last = br.bits(1)?;
        match br.bits(2)? {
            0 => {
                br.bit = 0;
                br.cnt = 0;
                let p = br.pos;
                if p + 4 > data.len() {
                    return Err("truncated stored block".into());
                }
                let len = u16::from_le_bytes([data[p], data[p + 1]]) as usize;
                let nlen = u16::from_le_bytes([data[p + 2], data[p + 3]]) as usize;
                if len != (!nlen & 0xffff) || p + 4 + len > data.len() {
                    return Err("bad stored block".into());
                }
                out.extend_from_slice(&data[p + 4..p + 4 + len]);
                br.pos = p + 4 + len;
            }
            1 => {
                let mut l = [0u8; 288];
                for (i, v) in l.iter_mut().enumerate() {
                    *v = if i < 144 {
                        8
                    } else if i < 256 {
                        9
                    } else if i < 280 {
                        7
                    } else {
                        8
                    };
                }
                codes(&mut br, &mut out, &build(&l), &build(&[5u8; 30]))?;
            }
            2 => {
                let nlen = br.bits(5)? as usize + 257;
                let ndist = br.bits(5)? as usize + 1;
                let ncode = br.bits(4)? as usize + 4;
                if nlen > 286 || ndist > 30 {
                    return Err("bad dynamic header".into());
                }
                let mut lengths = [0u8; 320];
                for &o in CODE_LENGTH_ORDER.iter().take(ncode) {
                    lengths[o] = br.bits(3)? as u8;
                }
                let lc = build(&lengths[..19]);
                let mut idx = 0;
                let total = nlen + ndist;
                let mut ll = [0u8; 320];
                while idx < total {
                    let sym = decode_sym(&mut br, &lc)?;
                    if sym < 16 {
                        ll[idx] = sym as u8;
                        idx += 1;
                    } else {
                        let (prev, rep) = match sym {
                            16 => {
                                if idx == 0 {
                                    return Err("bad repeat".into());
                                }
                                (ll[idx - 1], 3 + br.bits(2)? as usize)
                            }
                            17 => (0, 3 + br.bits(3)? as usize),
                            _ => (0, 11 + br.bits(7)? as usize),
                        };
                        if idx + rep > total {
                            return Err("too many lengths".into());
                        }
                        for _ in 0..rep {
                            ll[idx] = prev;
                            idx += 1;
                        }
                    }
                }
                codes(&mut br, &mut out, &build(&ll[..nlen]), &build(&ll[nlen..total]))?;
            }
            _ => return Err("bad block type".into()),
        }
        if last == 1 {
            return Ok(out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::inflate;

    #[test]
    fn raw_deflate_round_trips_and_bad_input_is_an_error() {
        let compressed: &[u8] = &[
            11, 72, 204, 204, 43, 73, 45, 82, 200, 78, 77, 45, 40, 86, 200, 44, 41, 86, 200, 77, 45, 206, 80, 72, 73,
            77, 203, 73, 44, 73, 77, 177, 86, 8, 24, 85, 48, 170, 128, 190, 10, 0,
        ];
        let expected = b"Painter keeps its mesh deflated; ".repeat(20);
        assert_eq!(inflate(compressed).as_deref(), Ok(expected.as_slice()));
        assert!(inflate(&[0xff, 0xff]).is_err());
        assert!(inflate(&[]).is_err());
    }
}
