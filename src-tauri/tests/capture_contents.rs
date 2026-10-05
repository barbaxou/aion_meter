//! What is actually inside a `saveRawPackets` capture.
//!
//! `packets_*.txt` is the whole server->client game connection, not a combat
//! feed, so the honest question for `docs/PRIVACY.md` is not "does it contain
//! combat data" but "what else is in there". This decompresses the LZ4 bundles
//! and reports the printable strings, which is the fastest way to see whether a
//! capture carries chat, mail, or bystanders' names.
//!
//! Diagnostic only — run it deliberately:
//!   A2_REPLAY_CAPTURE=... cargo test --test capture_contents -- --ignored --nocapture

use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    let b = hex.as_bytes();
    if b.len() % 2 != 0 {
        return None;
    }
    let mut out = Vec::with_capacity(b.len() / 2);
    for pair in b.chunks(2) {
        let hi = (pair[0] as char).to_digit(16)?;
        let lo = (pair[1] as char).to_digit(16)?;
        out.push(((hi << 4) | lo) as u8);
    }
    Some(out)
}

/// Pull every plain packet out of a buffer, decompressing bundles recursively.
fn flatten(buffer: &[u8], out: &mut Vec<Vec<u8>>, depth: usize) {
    if depth > 4 {
        return;
    }
    for frame in walk(buffer).frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let payload = frame.payload(buffer);
                if payload.len() < 7 {
                    continue;
                }
                let size =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
                if size == 0 || size > 1_000_000 {
                    continue;
                }
                if let Ok(inner) = lz4_flex::decompress(&payload[6..], size) {
                    flatten(&inner, out, depth + 1);
                }
            }
        }
    }
}

/// Runs of printable text long enough to be words rather than coincidence.
fn strings_in(data: &[u8], min: usize) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Vec<u8> = Vec::new();
    let mut flush = |cur: &mut Vec<u8>| {
        if cur.len() >= min {
            if let Ok(s) = std::str::from_utf8(cur) {
                out.push(s.to_string());
            }
        }
        cur.clear();
    };
    for &b in data {
        // ASCII printable, or a UTF-8 continuation/lead byte (CJK names).
        if (0x20..0x7F).contains(&b) || b >= 0xC0 || (0x80..0xC0).contains(&b) {
            cur.push(b);
        } else {
            flush(&mut cur);
        }
    }
    flush(&mut cur);
    out
}

#[test]
#[ignore = "diagnostic"]
fn what_is_in_a_capture() {
    let Ok(path) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE unset — skipping");
        return;
    };
    let text = std::fs::read_to_string(&path).expect("capture");

    let (mut lines, mut plain, mut bundled) = (0usize, 0usize, 0usize);
    let mut all_strings: Vec<String> = Vec::new();
    let mut opcode_hist: std::collections::HashMap<[u8; 2], usize> = Default::default();
    let mut streams: std::collections::HashMap<String, PacketAccumulator> =
        Default::default();
    // Horodatage de la première apparition de chaque opcode : c'est ce qui dit
    // si plusieurs paquets arrivent ensemble — donc sur une même action — ou
    // séparément.
    let mut premier_vu: std::collections::HashMap<[u8; 2], String> = Default::default();

    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let parts: Vec<&str> = line.splitn(3, '|').collect();
        if parts.len() != 3 {
            continue;
        }
        let Some(bytes) = decode_hex(parts[2]) else { continue };
        lines += 1;

        // Réassembler par flux, et ne consommer que les trames complètes.
        //
        // Cette boucle traitait chaque ligne séparément, et **c'était faux** :
        // une ligne est un morceau TCP d'environ 1 428 octets, alors que la
        // fiche en pèse 2 334 et les pets 3 647. Ces paquets-là sont à cheval
        // sur plusieurs lignes et n'étaient jamais vus. Mesuré le 05/10/2026 :
        // l'outil annonçait `33 36` et `00 90` absents d'une capture où le
        // meter, lui, venait de les lire et de les journaliser. Ce sont ces
        // deux témoins qui ont révélé le défaut de l'outil.
        let acc = streams
            .entry(parts[1].to_string())
            .or_insert_with(PacketAccumulator::new);
        acc.append(&bytes);
        let consumed = walk(acc.snapshot()).consumed;
        if consumed == 0 {
            continue;
        }
        let complet = acc.snapshot()[..consumed].to_vec();
        acc.discard_bytes(consumed);

        let before = {
            let mut v = Vec::new();
            flatten(&complet, &mut v, 0);
            v
        };
        for p in &before {
            // Opcode sits just past the length varint.
            let li = xiiinrv_meter_lib::capture::stream_processor::read_varint(p, 0);
            if li.length > 0 {
                let o = li.length as usize;
                if o + 1 < p.len() {
                    *opcode_hist.entry([p[o], p[o + 1]]).or_default() += 1;
                    premier_vu
                        .entry([p[o], p[o + 1]])
                        .or_insert_with(|| parts[0].to_string());
                }
            }
            all_strings.extend(strings_in(p, 6));
        }
        plain += before.len();
        bundled += 1;
    }

    println!("capture lines: {lines}, flattened packets: {plain}, buffers: {bundled}");

    let mut ops: Vec<_> = opcode_hist.into_iter().collect();
    ops.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
    println!("\ntop 25 leading opcodes:");
    for (op, n) in ops.iter().take(25) {
        println!("  {:02X} {:02X}  {n:>7}", op[0], op[1]);
    }

    // Les opcodes qui nous intéressent nommément, présents ou non.
    //
    // Un palmarès tronqué ne répond pas à la question « ce paquet est-il dans le
    // flux ? » : un opcode vu deux fois compte autant qu'un opcode absent s'il
    // tombe hors des 25 premiers. C'est la distinction entre « absent du flux »
    // et « présent mais non lu », et c'est elle qui dit où chercher.
    let attendus: [([u8; 2], &str); 8] = [
        ([0x33, 0x36], "PlayerInfo — notre fiche"),
        ([0x11, 0x56], "Inventaire — notre équipement"),
        ([0x56, 0x36], "Combat Power"),
        ([0x00, 0x90], "Pets"),
        ([0x02, 0x97], "PartyInfo — notre roster"),
        ([0x04, 0x38], "Damage"),
        ([0x05, 0x38], "DotDamage — non lu, mélangé au précédent"),
        ([0x49, 0x36], "PlayerStats — non lu"),
    ];
    println!("\nopcodes attendus :");
    for (op, quoi) in attendus {
        let n = ops.iter().find(|(o, _)| *o == op).map(|(_, n)| *n).unwrap_or(0);
        let etat = if n == 0 { "ABSENT " } else { "présent" };
        let quand = premier_vu.get(&op).map(String::as_str).unwrap_or("-");
        println!("  {:02X} {:02X}  {etat}  {n:>6}  {quand:<34}  {quoi}", op[0], op[1]);
    }

    // Dedupe and show the longest strings — chat and mail would surface here.
    all_strings.sort();
    all_strings.dedup();
    all_strings.sort_by_key(|s| std::cmp::Reverse(s.chars().count()));
    println!("\ndistinct printable runs >= 6 bytes: {}", all_strings.len());
    println!("longest 60:");
    for s in all_strings.iter().take(60) {
        let shown: String = s.chars().take(120).collect();
        println!("  [{:>3}] {}", s.chars().count(), shown.replace('\n', " "));
    }
}
