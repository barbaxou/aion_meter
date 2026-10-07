//! Que trouve-t-on exactement autour de la marque `11 56` dans nos paquets ?
//!
//! L'inventaire voyage imbriqué dans de plus gros paquets — mesuré le
//! 07/10/2026 : 4 occurrences à l'intérieur, aucune en tête, le plus gros
//! porteur faisant 32 764 octets. Pour le lire, il faut retrouver la tranche du
//! sous-paquet, et donc savoir comment sa longueur est écrite juste avant la
//! marque.
//!
//! Ce diagnostic ne suppose rien : il imprime les octets qui précèdent et qui
//! suivent chaque marque, et ce que donnerait une lecture de longueur sur un
//! puis deux octets. C'est à partir de là qu'on décide, pas avant.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test autour_de_la_marque -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

const MARQUE: [u8; 2] = [0x11, 0x56];

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

/// Un varint, comme `collecte::varint` le lit.
fn varint(d: &[u8], o: usize) -> Option<(u32, usize)> {
    let mut valeur: u32 = 0;
    let mut decalage = 0;
    let mut lus = 0;
    loop {
        let octet = *d.get(o + lus)?;
        lus += 1;
        valeur |= ((octet & 0x7F) as u32) << decalage;
        if octet & 0x80 == 0 {
            return Some((valeur, lus));
        }
        decalage += 7;
        if decalage >= 32 {
            return None;
        }
    }
}

#[test]
#[ignore = "diagnostic"]
fn que_trouve_t_on_autour_de_la_marque() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let mut flux: HashMap<String, PacketAccumulator> = HashMap::new();
    let mut paquets: Vec<(String, Vec<u8>)> = Vec::new();

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
        let mut v = Vec::new();
        aplatir(&complet, &mut v, 0);
        for p in v {
            paquets.push((bouts[0].to_string(), p));
        }
    }

    println!("\n{} paquets réassemblés", paquets.len());
    let mut vus = 0;
    for (ts, p) in &paquets {
        let mut i = 0usize;
        while i + 1 < p.len() {
            if p[i] != MARQUE[0] || p[i + 1] != MARQUE[1] {
                i += 1;
                continue;
            }
            vus += 1;
            let avant = &p[i.saturating_sub(8)..i];
            let apres = &p[i + 2..(i + 2 + 24).min(p.len())];
            println!(
                "\n  {}  marque à l'offset {} d'un paquet de {} octets",
                &ts[11..23.min(ts.len())],
                i,
                p.len()
            );
            println!("    8 octets avant : {}", avant.iter().map(|o| format!("{o:02x}")).collect::<Vec<_>>().join(" "));
            println!("    24 après       : {}", apres.iter().map(|o| format!("{o:02x}")).collect::<Vec<_>>().join(" "));
            for largeur in [1usize, 2, 3] {
                if i < largeur {
                    continue;
                }
                match varint(p, i - largeur) {
                    Some((v, lus)) if lus == largeur => {
                        println!("    longueur sur {largeur} octet(s) : {v}  (tient dans le paquet : {})", i - largeur + v as usize <= p.len());
                    }
                    _ => println!("    longueur sur {largeur} octet(s) : illisible"),
                }
            }
            i += 1;
        }
    }
    if vus == 0 {
        println!("  aucune marque trouvée");
    }
}
