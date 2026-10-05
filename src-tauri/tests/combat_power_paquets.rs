//! Que contiennent réellement les paquets `56 36` du Combat Power ?
//!
//! `lire_combat_power` ne lit qu'un champ, à l'offset 3, et exige **deux
//! paquets** dans la même fenêtre de cinq secondes pour retenir sa valeur.
//! Cette corroboration temporelle est justifiée — le 30/09/2026 un paquet
//! isolé parfaitement conforme annonçait 3 732 pour un personnage à 132 000 —
//! mais elle échoue quand un seul paquet passe, ce qui est arrivé le
//! 06/10/2026 : un unique `56 36` à 81 451, écarté, et un Combat Power resté
//! vide sur le site.
//!
//! Le §7 du document de reprise décrit une corroboration **structurelle** :
//! deux entiers de 64 bits consécutifs, tous deux dans une fourchette
//! plausible, le premier ≤ le second. Or la disposition annoncée de notre
//! paquet est précisément celle-là :
//!
//! ```text
//! 16 56 36 <valeur u32> 00 00 00 00 <second champ u32> 00 00 00 00
//! ```
//!
//! soit deux u64 petit-boutiens consécutifs à partir de l'offset 3. Ce
//! diagnostic imprime les deux champs de chaque paquet, pour savoir si le
//! second peut servir de second avis — avant d'écrire quoi que ce soit.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test combat_power_paquets -- --ignored --nocapture

use std::collections::HashMap;

use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};
use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
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
                    aplatir(&interne, out, profondeur + 1);
                }
            }
        }
    }
}

#[test]
#[ignore = "diagnostic"]
fn que_contiennent_les_paquets_de_combat_power() {
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

    let u32_le = |d: &[u8], o: usize| -> Option<u32> {
        d.get(o..o + 4)
            .map(|s| u32::from_le_bytes([s[0], s[1], s[2], s[3]]))
    };

    println!("\npaquets `56 36` trouvés dans {chemin} :");
    println!("  heure            taille  champ 1      champ 2      1<=2   tous deux dans 10k..2M");
    let mut vus = 0;
    for (ts, p) in &paquets {
        let li = xiiinrv_meter_lib::capture::stream_processor::read_varint(p, 0);
        if li.length == 0 {
            continue;
        }
        let o = li.length as usize;
        if o + 1 >= p.len() || p[o] != 0x56 || p[o + 1] != 0x36 {
            continue;
        }
        vus += 1;
        let (a, b) = (u32_le(p, 3), u32_le(p, 11));
        let bornes = |v: Option<u32>| v.is_some_and(|v| (10_000..=2_000_000).contains(&v));
        println!(
            "  {}  {:>5}   {:<11?}  {:<11?}  {:<5}  {}",
            &ts[11..23.min(ts.len())],
            p.len(),
            a,
            b,
            match (a, b) {
                (Some(x), Some(y)) => (x <= y).to_string(),
                _ => "?".to_string(),
            },
            bornes(a) && bornes(b)
        );
        println!("     octets : {}", p.iter().map(|o| format!("{o:02x}")).collect::<Vec<_>>().join(" "));
    }
    if vus == 0 {
        println!("  aucun");
    }
}
