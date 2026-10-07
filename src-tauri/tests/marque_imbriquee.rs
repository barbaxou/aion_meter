//! Les marques d'opcode apparaissent-elles **à l'intérieur** des paquets ?
//!
//! La fiche arrive imbriquée dans un plus gros paquet, et c'est établi : on l'a
//! vue portée par `60 88`, `47 4b` et `04 04`. `observer()` a donc un chemin de
//! secours pour elle — on cherche la marque `33 36` dans tout paquet, quel que
//! soit son opcode extérieur.
//!
//! **L'inventaire n'a aucun chemin de ce genre** : il n'est lu que si l'opcode
//! extérieur vaut exactement `11 56`. Si lui aussi arrive imbriqué, il est
//! invisible pour nous — ce qui expliquerait qu'il ne soit « jamais envoyé »
//! alors que l'équipement du personnage existe forcément quelque part.
//!
//! Ce diagnostic compte, pour chaque marque, combien de fois elle apparaît en
//! **tête** de paquet (donc lue aujourd'hui) et combien de fois **à
//! l'intérieur** d'un paquet plus gros (donc perdue, sauf chemin de secours).
//!
//! Il ne conclut pas : une paire d'octets peut apparaître par hasard dans
//! n'importe quelle charge. C'est pour cela que la fiche sert de témoin — on
//! sait qu'elle est réellement imbriquée, et on peut comparer les ordres de
//! grandeur.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test marque_imbriquee -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

const MARQUES: [([u8; 2], &str); 6] = [
    ([0x33, 0x36], "PlayerInfo — la fiche (témoin : on sait qu'elle s'imbrique)"),
    ([0x11, 0x56], "Inventaire — l'équipement"),
    ([0x56, 0x36], "Combat Power"),
    ([0x00, 0x90], "Pets"),
    ([0x02, 0x97], "PartyInfo — le roster"),
    ([0x49, 0x36], "PlayerStats"),
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
                    // `walk_inner`, et non `walk` : c'est ce que fait le meter
                    // pour le contenu décompressé. S'en écarter produisait de
                    // gros blocs fusionnés et faisait croire, le 07/10/2026,
                    // que nos paquets étaient dix fois trop gros.
                    aplatir_interne(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

/// L'en-tête, lu **exactement comme `collecte::entete`** : un varint de
/// longueur, puis deux octets d'opcode.
fn entete(p: &[u8]) -> Option<([u8; 2], usize)> {
    let mut lus = 0usize;
    loop {
        let octet = *p.get(lus)?;
        lus += 1;
        if octet & 0x80 == 0 {
            break;
        }
        if lus >= 5 {
            return None;
        }
    }
    Some(([*p.get(lus)?, *p.get(lus + 1)?], lus + 2))
}

#[test]
#[ignore = "diagnostic"]
fn ou_apparaissent_les_marques() {
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

    println!("\n{} paquets réassemblés depuis {chemin}", paquets.len());
    println!("\n  marque   en tête   à l'intérieur   plus gros porteur   quoi");

    for (m, quoi) in MARQUES {
        let mut en_tete = 0usize;
        let mut dedans = 0usize;
        let mut plus_gros = 0usize;
        for p in &paquets {
            let tete = entete(p).map(|(op, _)| op == m).unwrap_or(false);
            if tete {
                en_tete += 1;
                continue;
            }
            // La marque ailleurs qu'en tête : le paquet la porte sans la déclarer.
            if p.windows(2).any(|f| f == m) {
                dedans += 1;
                plus_gros = plus_gros.max(p.len());
            }
        }
        println!(
            "  {:02X} {:02X}   {:>5}   {:>11}   {:>15}   {}",
            m[0], m[1], en_tete, dedans, plus_gros, quoi
        );
    }
}
