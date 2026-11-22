//! Pure Rust DES ECB Decryptor with zero external crypto dependencies.

use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine;
use std::sync::OnceLock;

const IP: [usize; 64] = [
    58, 50, 42, 34, 26, 18, 10, 2, 60, 52, 44, 36, 28, 20, 12, 4, 62, 54, 46, 38, 30, 22, 14, 6,
    64, 56, 48, 40, 32, 24, 16, 8, 57, 49, 41, 33, 25, 17, 9, 1, 59, 51, 43, 35, 27, 19, 11, 3,
    61, 53, 45, 37, 29, 21, 13, 5, 63, 55, 47, 39, 31, 23, 15, 7,
];

const FP: [usize; 64] = [
    40, 8, 48, 16, 56, 24, 64, 32, 39, 7, 47, 15, 55, 23, 63, 31, 38, 6, 46, 14, 54, 22, 62, 30,
    37, 5, 45, 13, 53, 21, 61, 29, 36, 4, 44, 12, 52, 20, 60, 28, 35, 3, 43, 11, 51, 19, 59, 27,
    34, 2, 42, 10, 50, 18, 58, 26, 33, 1, 41, 9, 49, 17, 57, 25,
];

const E: [usize; 48] = [
    32, 1, 2, 3, 4, 5, 4, 5, 6, 7, 8, 9, 8, 9, 10, 11, 12, 13, 12, 13, 14, 15, 16, 17, 16, 17, 18,
    19, 20, 21, 20, 21, 22, 23, 24, 25, 24, 25, 26, 27, 28, 29, 28, 29, 30, 31, 32, 1,
];

const P: [usize; 32] = [
    16, 7, 20, 21, 29, 12, 28, 17, 1, 15, 23, 26, 5, 18, 31, 10, 2, 8, 24, 14, 32, 27, 3, 9, 19,
    13, 30, 6, 22, 11, 4, 25,
];

const S: [[[u8; 16]; 4]; 8] = [
    [
        [14, 4, 13, 1, 2, 15, 11, 8, 3, 10, 6, 12, 5, 9, 0, 7],
        [0, 15, 7, 4, 14, 2, 13, 1, 10, 6, 12, 11, 9, 5, 3, 8],
        [4, 1, 14, 8, 13, 6, 2, 11, 15, 12, 9, 7, 3, 10, 5, 0],
        [15, 12, 8, 2, 4, 9, 1, 7, 5, 11, 3, 14, 10, 0, 6, 13],
    ],
    [
        [15, 1, 8, 14, 6, 11, 3, 4, 9, 7, 2, 13, 12, 0, 5, 10],
        [3, 13, 4, 7, 15, 2, 8, 14, 12, 0, 1, 10, 6, 9, 11, 5],
        [0, 14, 7, 11, 10, 4, 13, 1, 5, 8, 12, 6, 9, 3, 2, 15],
        [13, 8, 10, 1, 3, 15, 4, 2, 11, 6, 7, 12, 0, 5, 14, 9],
    ],
    [
        [10, 0, 9, 14, 6, 3, 15, 5, 1, 13, 12, 7, 11, 4, 2, 8],
        [13, 7, 0, 9, 3, 4, 6, 10, 2, 8, 5, 14, 12, 11, 15, 1],
        [13, 6, 4, 9, 8, 15, 3, 0, 11, 1, 2, 12, 5, 10, 14, 7],
        [1, 10, 13, 0, 6, 9, 8, 7, 4, 15, 14, 3, 11, 5, 2, 12],
    ],
    [
        [7, 13, 14, 3, 0, 6, 9, 10, 1, 2, 8, 5, 11, 12, 4, 15],
        [13, 8, 11, 5, 6, 15, 0, 3, 4, 7, 2, 12, 1, 10, 14, 9],
        [10, 6, 9, 0, 12, 11, 7, 13, 15, 1, 3, 14, 5, 2, 8, 4],
        [3, 15, 0, 6, 10, 1, 13, 8, 9, 4, 5, 11, 12, 7, 2, 14],
    ],
    [
        [2, 12, 4, 1, 7, 10, 11, 6, 8, 5, 3, 15, 13, 0, 14, 9],
        [14, 11, 2, 12, 4, 7, 13, 1, 5, 0, 15, 10, 3, 9, 8, 6],
        [4, 2, 1, 11, 10, 13, 7, 8, 15, 9, 12, 5, 6, 3, 0, 14],
        [11, 8, 12, 7, 1, 14, 2, 13, 6, 15, 0, 9, 10, 4, 5, 3],
    ],
    [
        [12, 1, 10, 15, 9, 2, 6, 8, 0, 13, 3, 4, 14, 7, 5, 11],
        [10, 15, 4, 2, 7, 12, 9, 5, 6, 1, 13, 14, 0, 11, 3, 8],
        [9, 14, 15, 5, 2, 8, 12, 3, 7, 0, 4, 10, 1, 13, 11, 6],
        [4, 3, 2, 12, 9, 5, 15, 10, 11, 14, 1, 7, 6, 0, 8, 13],
    ],
    [
        [4, 11, 2, 14, 15, 0, 8, 13, 3, 12, 9, 7, 5, 10, 6, 1],
        [13, 0, 11, 7, 4, 9, 1, 10, 14, 3, 5, 12, 2, 15, 8, 6],
        [1, 4, 11, 13, 12, 3, 7, 14, 10, 15, 6, 8, 0, 5, 9, 2],
        [6, 11, 13, 8, 1, 4, 10, 7, 9, 5, 0, 15, 14, 2, 3, 12],
    ],
    [
        [13, 2, 8, 4, 6, 15, 11, 1, 10, 9, 3, 14, 5, 0, 12, 7],
        [1, 15, 13, 8, 10, 3, 7, 4, 12, 5, 6, 11, 0, 14, 9, 2],
        [7, 11, 4, 1, 9, 12, 14, 2, 0, 6, 10, 13, 15, 3, 5, 8],
        [2, 1, 14, 7, 4, 10, 8, 13, 15, 12, 9, 0, 3, 5, 6, 11],
    ],
];

const PC1: [usize; 56] = [
    57, 49, 41, 33, 25, 17, 9, 1, 58, 50, 42, 34, 26, 18, 10, 2, 59, 51, 43, 35, 27, 19, 11, 3,
    60, 52, 44, 36, 63, 55, 47, 39, 31, 23, 15, 7, 62, 54, 46, 38, 30, 22, 14, 6, 61, 53, 45, 37,
    29, 21, 13, 5, 28, 20, 12, 4,
];

const PC2: [usize; 48] = [
    14, 17, 11, 24, 1, 5, 3, 28, 15, 6, 21, 10, 23, 19, 12, 4, 26, 8, 16, 7, 27, 20, 13, 2, 41,
    52, 31, 37, 47, 55, 30, 40, 51, 45, 33, 48, 44, 49, 39, 56, 34, 53, 46, 42, 50, 36, 29, 32,
];

const SHIFTS: [usize; 16] = [1, 1, 2, 2, 2, 2, 2, 2, 1, 2, 2, 2, 2, 2, 2, 1];

fn bytes_to_bits(bytes: &[u8]) -> Vec<u8> {
    let mut bits = Vec::with_capacity(bytes.len() * 8);
    for &b in bytes {
        for j in (0..8).rev() {
            bits.push((b >> j) & 1);
        }
    }
    bits
}

fn bits_to_bytes(bits: &[u8]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(bits.len() / 8);
    for chunk in bits.chunks_exact(8) {
        let mut b = 0u8;
        for &bit in chunk {
            b = (b << 1) | bit;
        }
        bytes.push(b);
    }
    bytes
}

fn permute(bits: &[u8], table: &[usize]) -> Vec<u8> {
    table.iter().map(|&pos| bits[pos - 1]).collect()
}

static DECRYPT_SUBKEYS: OnceLock<Vec<Vec<u8>>> = OnceLock::new();

fn get_decrypt_subkeys() -> &'static [Vec<u8>] {
    DECRYPT_SUBKEYS.get_or_init(|| {
        let key_bits = bytes_to_bits(b"38346591");
        let pc1_bits = permute(&key_bits, &PC1);
        let mut c: Vec<u8> = pc1_bits[..28].to_vec();
        let mut d: Vec<u8> = pc1_bits[28..56].to_vec();
        let mut subkeys: Vec<Vec<u8>> = Vec::with_capacity(16);

        for r in 0..16 {
            let s = SHIFTS[r];
            c.rotate_left(s);
            d.rotate_left(s);
            let mut combined = Vec::with_capacity(56);
            combined.extend_from_slice(&c);
            combined.extend_from_slice(&d);
            subkeys.push(permute(&combined, &PC2));
        }

        subkeys.reverse();
        subkeys
    })
}

fn feistel(r: &[u8], subkey: &[u8]) -> Vec<u8> {
    let expanded = permute(r, &E);
    let xored: Vec<u8> = expanded.iter().zip(subkey.iter()).map(|(&b, &k)| b ^ k).collect();
    let mut s_output = Vec::with_capacity(32);

    for (i, chunk) in xored.chunks_exact(6).enumerate() {
        let row = ((chunk[0] << 1) | chunk[5]) as usize;
        let col = ((chunk[1] << 3) | (chunk[2] << 2) | (chunk[3] << 1) | chunk[4]) as usize;
        let val = S[i][row][col];
        for j in (0..4).rev() {
            s_output.push((val >> j) & 1);
        }
    }

    permute(&s_output, &P)
}

/// Decrypts base64 DES-ECB encrypted media tokens using fixed key '38346591'.
pub fn decrypt_saavn_media_url(base64_str: &str) -> String {
    let trimmed = base64_str.trim();
    if trimmed.is_empty() {
        return String::new();
    }

    let cipher_bytes = match BASE64_STANDARD.decode(trimmed) {
        Ok(b) if b.len() % 8 == 0 && !b.is_empty() => b,
        _ => return String::new(),
    };

    let subkeys = get_decrypt_subkeys();
    let mut out_bits = Vec::with_capacity(cipher_bytes.len() * 8);

    for block in cipher_bytes.chunks_exact(8) {
        let blk_bits = bytes_to_bits(block);
        let ip_bits = permute(&blk_bits, &IP);
        let mut l = ip_bits[..32].to_vec();
        let mut r = ip_bits[32..64].to_vec();

        for r_idx in 0..16 {
            let f = feistel(&r, &subkeys[r_idx]);
            let next_l = r;
            let next_r: Vec<u8> = l.iter().zip(f.iter()).map(|(&a, &b)| a ^ b).collect();
            l = next_l;
            r = next_r;
        }

        let mut combined = Vec::with_capacity(64);
        combined.extend_from_slice(&r);
        combined.extend_from_slice(&l);
        let fp_bits = permute(&combined, &FP);
        out_bits.extend(fp_bits);
    }

    let dec_bytes = bits_to_bytes(&out_bits);
    if dec_bytes.is_empty() {
        return String::new();
    }

    let pad_len = dec_bytes[dec_bytes.len() - 1] as usize;
    let unpadded = if pad_len > 0 && pad_len <= 8 && pad_len <= dec_bytes.len() {
        &dec_bytes[..dec_bytes.len() - pad_len]
    } else {
        &dec_bytes[..]
    };

    String::from_utf8_lossy(unpadded).trim().to_string()
}
