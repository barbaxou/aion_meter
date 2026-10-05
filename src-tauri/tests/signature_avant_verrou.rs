//! Les paquets de la fiche survivraient-ils au filtre d'avant-verrouillage ?
//!
//! Le dispatcher jette tout paquet serveur→client qui ne porte aucune des deux
//! signatures de combat, **tant que le port n'est pas verrouillé** :
//!
//! ```text
//! let unlocked = current_port.is_none();
//! if unlocked && !contains_any(&cap.data, &COMBAT_SIGNATURES) {
//!     continue;
//! }
//! ```
//!
//! Or le verrou exige douze paquets porteurs de signature en trois secondes,
//! cadence que seul le flux en monde produit. À la connexion, le meter n'est
//! donc pas encore verrouillé. Si les paquets de la fiche, de l'inventaire et
//! du Combat Power ne portent pas de signature, ils sont jetés à ce
//! moment-là — et c'est précisément ceux-là qui n'arrivent jamais chez
//! barbaxou, dont l'équipement et le Combat Power.
//!
//! Ce diagnostic le mesure sur un enregistrement qui, lui, les contient.
//! Il ne conclut pas à notre place : il compte, pour chaque opcode, combien de
//! ses paquets porteraient une signature.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test signature_avant_verrou -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

/// Les deux signatures de `COMBAT_SIGNATURES`, recopiées depuis le dispatcher.
const SIGNATURES: [&[u8]; 2] = [
    &[0x0E, 0x00, 0x36], // terminateur d'enregistrement actuel
    &[0x06, 0x00, 0x36], // terminateur d'avant juin 2026
];

const INTERESSANTS: [([u8; 2], &str); 6] = [
    ([0x33, 0x36], "PlayerInfo — la fiche"),
    ([0x11, 0x56], "Inventaire — l'équipement"),
    ([0x56, 0x36], "Combat Power"),
    ([0x00, 0x90], "Pets"),
    ([0x49, 0x36], "PlayerStats"),
    ([0x04, 0x38], "Damage — témoin : doit être signé"),
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

/// Tous les paquets d'un tampon, lots décompressés compris.
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
                    aplatir(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

fn porte_une_signature(p: &[u8]) -> bool {
    SIGNATURES
        .iter()
        .any(|s| p.windows(s.len()).any(|f| f == *s))
}

fn opcode(p: &[u8]) -> Option<[u8; 2]> {
    let li = xiiinrv_meter_lib::capture::stream_processor::read_varint(p, 0);
    if li.length == 0 {
        return None;
    }
    let o = li.length as usize;
    (o + 1 < p.len()).then(|| [p[o], p[o + 1]])
}

#[test]
#[ignore = "diagnostic"]
fn les_paquets_de_la_fiche_portent_ils_une_signature() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    // Réassembler par flux, comme le meter : une ligne est un morceau TCP, et
    // la fiche comme l'inventaire sont plus gros qu'un morceau.
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

    // opcode -> (paquets vus, paquets portant une signature)
    let mut compte: HashMap<[u8; 2], (usize, usize)> = HashMap::new();
    for p in &paquets {
        if let Some(op) = opcode(p) {
            let e = compte.entry(op).or_insert((0, 0));
            e.0 += 1;
            if porte_une_signature(p) {
                e.1 += 1;
            }
        }
    }

    println!("\n{} paquets réassemblés depuis {}", paquets.len(), chemin);
    println!("\nsurvivraient-ils au filtre d'avant-verrouillage ?");
    println!("  opcode  signés/vus  verdict        quoi");
    for (op, quoi) in INTERESSANTS {
        let (vus, signes) = compte.get(&op).copied().unwrap_or((0, 0));
        let verdict = if vus == 0 {
            "absent"
        } else if signes == 0 {
            "TOUS JETÉS"
        } else if signes == vus {
            "tous gardés"
        } else {
            "en partie"
        };
        println!(
            "  {:02X} {:02X}   {:>4}/{:<5}  {:<13}  {}",
            op[0], op[1], signes, vus, verdict, quoi
        );
    }

    // Vue d'ensemble : la proportion sur tout le flux.
    let total = paquets.len();
    let signes = paquets.iter().filter(|p| porte_une_signature(p)).count();
    println!(
        "\nsur l'ensemble : {signes}/{total} paquets porteraient une signature \
         ({:.0} % seraient jetés avant verrouillage)",
        100.0 * (total - signes) as f64 / total.max(1) as f64
    );
}
