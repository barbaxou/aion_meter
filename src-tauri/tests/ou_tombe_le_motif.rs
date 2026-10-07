//! Où le motif `0E 00 36` tombe-t-il par rapport aux frontières de paquets ?
//!
//! Kuroukihime ancre son flux TCP sur ce motif et s'y réancre dès qu'il le perd
//! — ses journaux le disent : « Packet Stream SYNCHRONIZED on pattern
//! 06-00-36 », cinquante fois en deux jours. Notre découpage, lui, avance d'un
//! octet sur une longueur aberrante et réessaie : il retombe par hasard sur une
//! longueur plausible et émet un faux paquet qui avale les vrais. Mesuré le
//! 07/10/2026 : dix blocs de 8 à 32 Ko absorbaient 11 % de nos octets.
//!
//! Pour nous réancrer comme lui, il faut savoir **ce que le motif marque**. Le
//! code le documente comme un terminateur d'enregistrement de combat, donc une
//! position *à l'intérieur* d'un paquet — ce qui ne donne pas son début.
//!
//! Ce diagnostic ne suppose rien : pour chaque paquet correctement découpé, il
//! relève la position du motif depuis le **début** et depuis la **fin**. Si
//! l'une des deux est constante, la règle d'ancrage est acquise et
//! l'implémentation devient mécanique. Sinon, il ne faut pas l'écrire.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test ou_tombe_le_motif -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

const MOTIFS: [(&[u8], &str); 2] = [
    (&[0x0E, 0x00, 0x36], "0E 00 36 (actuel)"),
    (&[0x06, 0x00, 0x36], "06 00 36 (ancien, celui de Kuroukihime)"),
];

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

fn aplatir_interne(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize) {
    if profondeur > 4 {
        return;
    }
    for frame in walk_inner(buffer).frames {
        match frame.kind {
            FrameKind::Packet => out.push(frame.bytes(buffer).to_vec()),
            FrameKind::Bundle => {
                let payload = frame.payload(buffer);
                if payload.len() < 7 {
                    continue;
                }
                let taille =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
                if taille == 0 || taille > 1_000_000 {
                    continue;
                }
                if let Ok(interne) = lz4_flex::decompress(&payload[6..], taille) {
                    aplatir_interne(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

fn aplatir(buffer: &[u8], out: &mut Vec<Vec<u8>>, profondeur: usize) {
    if profondeur > 4 {
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
                let taille =
                    u32::from_le_bytes([payload[2], payload[3], payload[4], payload[5]]) as usize;
                if taille == 0 || taille > 1_000_000 {
                    continue;
                }
                if let Ok(interne) = lz4_flex::decompress(&payload[6..], taille) {
                    aplatir_interne(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

#[test]
#[ignore = "diagnostic"]
fn ou_tombe_le_motif_dans_un_paquet() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let mut flux: HashMap<String, PacketAccumulator> = HashMap::new();
    let mut paquets: Vec<Vec<u8>> = Vec::new();
    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        let Some(octets) = decode_hex(bouts[2]) else { continue };
        let acc = flux
            .entry(bouts[1].to_string())
            .or_insert_with(PacketAccumulator::new);
        acc.append(&octets);
        let consomme = walk(acc.snapshot()).consumed;
        if consomme == 0 {
            continue;
        }
        let complet = acc.snapshot()[..consomme].to_vec();
        acc.discard_bytes(consomme);
        aplatir(&complet, &mut paquets, 0);
    }

    // Où le découpage a perdu l'alignement, compté par le produit lui-même.
    println!(
        "
{}",
        xiiinrv_meter_lib::capture::framing::diag_reprises::bilan()
    );

    // Seuls les paquets de taille plausible : au-delà, c'est un faux paquet
    // produit par un désalignement, et il fausserait la mesure.
    let plausibles: Vec<&Vec<u8>> = paquets.iter().filter(|p| p.len() <= 8192).collect();
    println!(
        "\n{} paquets, dont {} de taille plausible (<= 8192 o)",
        paquets.len(),
        plausibles.len()
    );

    for (motif, nom) in MOTIFS {
        let mut depuis_debut: HashMap<usize, usize> = HashMap::new();
        let mut depuis_fin: HashMap<usize, usize> = HashMap::new();
        let mut porteurs = 0usize;
        let mut occurrences = 0usize;

        for p in &plausibles {
            let mut vu = false;
            let mut i = 0usize;
            while i + motif.len() <= p.len() {
                if &p[i..i + motif.len()] == motif {
                    vu = true;
                    occurrences += 1;
                    *depuis_debut.entry(i).or_default() += 1;
                    *depuis_fin.entry(p.len() - i).or_default() += 1;
                }
                i += 1;
            }
            if vu {
                porteurs += 1;
            }
        }

        println!("\n{nom}");
        println!("  {porteurs} paquets le portent, {occurrences} occurrences");
        if occurrences == 0 {
            continue;
        }
        let top = |m: &HashMap<usize, usize>| -> String {
            let mut v: Vec<(&usize, &usize)> = m.iter().collect();
            v.sort_by_key(|(_, n)| std::cmp::Reverse(**n));
            v.iter()
                .take(6)
                .map(|(pos, n)| {
                    format!("{pos} ({:.0} %)", 100.0 * **n as f64 / occurrences as f64)
                })
                .collect::<Vec<_>>()
                .join("  ")
        };
        println!("  positions depuis le début : {}", top(&depuis_debut));
        println!("  positions depuis la fin   : {}", top(&depuis_fin));
        println!(
            "  → {} positions distinctes depuis le début, {} depuis la fin",
            depuis_debut.len(),
            depuis_fin.len()
        );
    }
}
