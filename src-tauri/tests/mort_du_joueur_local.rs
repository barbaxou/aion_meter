//! Quel paquet signale la mort du joueur local ?
//!
//! Mesuré le 16/10/2026 : Abyss DPS Meter compte deux morts pour Barbax
//! (20:57:22.900 et 20:57:59.798) ; notre compteur, fondé sur les PV à zéro,
//! n'en voit aucune — la vie du joueur local ne tombe jamais à zéro dans le
//! flux. Ce diagnostic cherche, autour de ces deux instants, les opcodes qui
//! n'apparaissent presque nulle part ailleurs.
//!
//! A2_REPLAY_CAPTURE=... A2_MORTS_A=20:57:22.9,20:57:59.8 \
//!   cargo test --test mort_du_joueur_local -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
use xiiinrv_meter_lib::capture::stream_processor::read_varint;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 { return None; }
    (0..hex.len()).step_by(2).map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok()).collect()
}

fn aplatir(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize, interne: bool) {
    if profondeur > 4 { return; }
    let d = if interne { walk_inner(buffer) } else { walk(buffer) };
    for frame in d.frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let p = frame.payload(buffer);
                if p.len() < 7 { continue; }
                let taille = u32::from_le_bytes([p[2], p[3], p[4], p[5]]) as usize;
                if taille == 0 || taille > 1_000_000 { continue; }
                if let Ok(int) = lz4_flex::decompress(&p[6..], taille) {
                    aplatir(&int, out, profondeur + 1, true);
                }
            }
        }
    }
}

/// « 20:57:22.900 » -> millisecondes depuis minuit.
fn ms(h: &str) -> i64 {
    let (hms, frac) = h.split_once('.').unwrap_or((h, "0"));
    let p: Vec<i64> = hms.split(':').map(|x| x.parse().unwrap()).collect();
    let f: i64 = format!("{:0<3}", frac)[..3].parse().unwrap();
    ((p[0] * 60 + p[1]) * 60 + p[2]) * 1000 + f
}

#[test]
#[ignore = "diagnostic"]
fn paquets_autour_des_morts_du_joueur_local() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else { return };
    let instants: Vec<i64> = std::env::var("A2_MORTS_A").unwrap_or_default()
        .split(',').filter(|s| !s.is_empty()).map(ms).collect();
    let texte = std::fs::read_to_string(&chemin).expect("capture lisible");

    let mut accs: HashMap<String, PacketAccumulator> = HashMap::new();
    let mut total: HashMap<(u8, u8), usize> = HashMap::new();
    let mut fenetres: Vec<HashMap<(u8, u8), Vec<(i64, usize, String)>>> = vec![HashMap::new(); instants.len()];

    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') { continue; }
        let b: Vec<&str> = ligne.splitn(3, '|').collect();
        if b.len() != 3 || !b[1].starts_with("Client") { continue; }
        // 2026-10-16T20:57:22.074237+02:00
        let Some(h) = b[0].get(11..23) else { continue };
        let t = ms(h);
        let Some(donnees) = decode_hex(b[2]) else { continue };
        let acc = accs.entry(b[1].to_string()).or_insert_with(PacketAccumulator::new);
        acc.append(&donnees);
        let tampon = acc.snapshot().to_vec();
        let mut paquets = Vec::new();
        aplatir(&tampon, &mut paquets, 0, false);
        let d = walk(&tampon);
        if d.consumed > 0 { acc.discard_bytes(d.consumed); }
        for p in paquets {
            let li = read_varint(&p, 0);
            if li.length < 0 { continue; }
            let o = li.length as usize;
            if o + 1 >= p.len() { continue; }
            let op = (p[o], p[o + 1]);
            *total.entry(op).or_insert(0) += 1;
            for (i, &c) in instants.iter().enumerate() {
                if (t - c).abs() <= 1500 {
                    let hex: String = p.iter().take(40).map(|x| format!("{x:02x}")).collect();
                    fenetres[i].entry(op).or_default().push((t - c, p.len(), hex));
                }
            }
        }
    }

    for (i, f) in fenetres.iter().enumerate() {
        println!("\n=== fenêtre {i} : ±1,5 s autour de l'instant {} ===", instants[i]);
        let mut v: Vec<_> = f.iter().collect();
        v.sort_by_key(|(op, _)| total[*op]);
        for (op, liste) in v.iter().take(25) {
            let (dt, len, hex) = &liste[0];
            println!("{:02x} {:02x}  ({:>5} au total, {} ici)  premier à {:+}ms, {} octets : {}",
                op.0, op.1, total[*op], liste.len(), dt, len, hex);
        }
    }
}
