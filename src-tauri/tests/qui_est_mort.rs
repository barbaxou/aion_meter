//! Le compteur de morts voit-il quelque chose dans une vraie session ?
//!
//! Ajouté en 2.0.68, il n'affichait rien pendant le donjon du 07/10/2026 alors
//! qu'un meter tiers comptait quatre joueurs à une mort et un à deux. Trois
//! explications possibles, et il faut les départager :
//!
//! 1. le paquet de mort n'est pas lu du tout ;
//! 2. il est lu, mais le drapeau `3` (mort au combat) ne tombe jamais ;
//! 3. il est lu et compté, mais sur une entité qui n'est pas la ligne du
//!    joueur — auquel cas le compte existe et personne ne le regarde.
//!
//! Ce rejeu lit une capture réelle dans le vrai chemin de lecture, puis dit
//! qui est mort, combien de fois, et si ces entités portent un pseudo.
//!
//! Diagnostic seulement — à lancer délibérément :
//!   A2_REPLAY_CAPTURE=... cargo test --test qui_est_mort -- --ignored --nocapture

use std::collections::HashMap;
use std::sync::Arc;

use xiiinrv_meter_lib::capture::stream_assembler::StreamAssembler;
use xiiinrv_meter_lib::capture::stream_processor::StreamProcessor;
use xiiinrv_meter_lib::combat::data_storage::DataStorage;
use xiiinrv_meter_lib::i18n::lookup::{NpcLookup, SkillLookup};

fn decode_hex(hex: &str) -> Option<Vec<u8>> {
    if hex.len() % 2 != 0 {
        return None;
    }
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).ok())
        .collect()
}

#[test]
#[ignore = "diagnostic"]
fn qui_est_mort_dans_cette_capture() {
    let Ok(chemin) = std::env::var("A2_REPLAY_CAPTURE") else {
        eprintln!("A2_REPLAY_CAPTURE non défini — test ignoré");
        return;
    };
    let texte = std::fs::read_to_string(&chemin).expect("enregistrement lisible");

    let storage = Arc::new(DataStorage::new());
    let mut flux: HashMap<String, (StreamAssembler, StreamProcessor)> = HashMap::new();
    let mut lignes = 0usize;

    // Combien de fois les octets d'un paquet de mort apparaissent, avant tout
    // décodage : si c'est zéro, l'explication 1 tient et le reste est inutile.
    let (mut vus_41, mut vus_42) = (0usize, 0usize);

    for ligne in texte.lines() {
        let ligne = ligne.trim();
        if ligne.is_empty() || ligne.starts_with('#') {
            continue;
        }
        let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
        if bouts.len() != 3 {
            continue;
        }
        let Some(donnees) = decode_hex(bouts[2]) else { continue };
        lignes += 1;
        for f in donnees.windows(2) {
            if f[1] == 0x36 && f[0] == 0x41 {
                vus_41 += 1;
            }
            if f[1] == 0x36 && f[0] == 0x42 {
                vus_42 += 1;
            }
        }

        let (assembleur, processeur) = flux.entry(bouts[1].to_string()).or_insert_with(|| {
            (
                StreamAssembler::new(),
                StreamProcessor::new(
                    storage.clone(),
                    Arc::new(SkillLookup::new()),
                    Arc::new(NpcLookup::new()),
                ),
            )
        });
        assembleur.process_chunk(&donnees, processeur);
    }

    // Deuxième passe : on décode nous-mêmes **tous** les paquets de mort, sans
    // filtrer sur le drapeau, pour savoir si les joueurs y figurent et sous
    // quel drapeau. Le produit ne retient que le drapeau 3.
    let pseudos = storage.get_nicknames();
    {
        use xiiinrv_meter_lib::capture::framing::{walk, FrameKind};
        use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
        use xiiinrv_meter_lib::capture::stream_processor::read_varint;
        let mut par_drapeau: HashMap<i32, usize> = HashMap::new();
        let mut joueurs_vus: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut accs: HashMap<String, PacketAccumulator> = HashMap::new();
        for ligne in texte.lines() {
            let ligne = ligne.trim();
            if ligne.is_empty() || ligne.starts_with('#') { continue; }
            let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
            if bouts.len() != 3 { continue; }
            let Some(donnees) = decode_hex(bouts[2]) else { continue };
            let acc = accs.entry(bouts[1].to_string()).or_insert_with(PacketAccumulator::new);
            acc.append(&donnees);
            let tampon = acc.snapshot().to_vec();
            let d = walk(&tampon);
            for frame in &d.frames {
                if frame.kind != FrameKind::Packet { continue; }
                let paquet = frame.bytes(&tampon);
                let li = read_varint(paquet, 0);
                if li.length < 0 { continue; }
                let o = li.length as usize;
                if o + 1 >= paquet.len() { continue; }
                if paquet[o + 1] != 0x36 || (paquet[o] != 0x41 && paquet[o] != 0x42) { continue; }
                let mut pos = o + 2;
                let ent = read_varint(paquet, pos);
                if ent.length <= 0 { continue; }
                pos += ent.length as usize;
                let saut = read_varint(paquet, pos);
                if saut.length <= 0 { continue; }
                pos += saut.length as usize;
                let dr = read_varint(paquet, pos);
                if dr.length <= 0 { continue; }
                *par_drapeau.entry(dr.value).or_insert(0) += 1;
                if pseudos.contains_key(&ent.value) {
                    joueurs_vus.entry(ent.value).or_default().push(dr.value);
                }
            }
            if d.consumed > 0 { acc.discard_bytes(d.consumed); }
        }
        let mut dr: Vec<(i32, usize)> = par_drapeau.into_iter().collect();
        dr.sort_by_key(|(_, n)| std::cmp::Reverse(*n));
        println!("
paquets de mort décodés, par drapeau :");
        for (v, n) in dr.iter().take(8) {
            println!("  drapeau {v:>3} : {n:>6}{}", if *v == 3 { "   <- le seul que le produit retient" } else { "" });
        }
        println!("
{} entités portant un pseudo apparaissent dans ce flux :", joueurs_vus.len());
        for (id, drapeaux) in joueurs_vus.iter().take(12) {
            println!("  {id:>9}  {:<16}  drapeaux {:?}", pseudos.get(id).cloned().unwrap_or_default(), drapeaux);
        }
    }
    let morts = storage.get_dead_entities();
    let combat = storage.get_combat_snapshot_light();

    println!("\n{lignes} lignes rejouées depuis {chemin}");
    println!("\nmotifs bruts dans le flux (avant décodage) :");
    println!("  41 36 (ancien opcode de mort) : {vus_41}");
    println!("  42 36 (opcode depuis 06/2026) : {vus_42}");

    println!("\n{} entités marquées mortes :", morts.len());
    let mut lignes_mortes: Vec<(i32, u32, String, bool)> = morts
        .iter()
        .map(|&id| {
            let nom = pseudos.get(&id).cloned().unwrap_or_default();
            let frappeur = combat.values().any(|td| td.actors.contains_key(&id));
            (id, storage.morts(id), nom, frappeur)
        })
        .collect();
    lignes_mortes.sort_by_key(|(_, n, _, _)| std::cmp::Reverse(*n));
    println!("  {:>9}  {:>6}  {:<18}  {}", "entité", "morts", "pseudo", "a frappé ?");
    for (id, n, nom, frappeur) in lignes_mortes.iter().take(40) {
        println!(
            "  {id:>9}  {n:>6}  {:<18}  {}",
            if nom.is_empty() { "—" } else { nom.as_str() },
            if *frappeur { "oui" } else { "non" }
        );
    }

    // Que valaient les PV juste avant chaque chute à zéro ?
    //
    // Le balayage du flux `8D` cherche un motif d'octets. Sur un identifiant
    // court il tombe juste par hasard, et chaque fausse lecture comptait une
    // mort — 31 pour un joueur qui en avait une. Si les vraies morts suivent
    // des PV bas et les fausses des PV pleins, la règle est trouvée.
    {
        use xiiinrv_meter_lib::capture::framing::{walk, walk_inner, FrameKind};
        use xiiinrv_meter_lib::capture::packet_accumulator::PacketAccumulator;
        use xiiinrv_meter_lib::capture::stream_processor::read_varint;

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

        let suivis: Vec<i32> = pseudos.keys().copied().collect();
        let mut series: HashMap<i32, Vec<i32>> = HashMap::new();
        let mut accs: HashMap<String, PacketAccumulator> = HashMap::new();
        for ligne in texte.lines() {
            let ligne = ligne.trim();
            if ligne.is_empty() || ligne.starts_with('#') { continue; }
            let bouts: Vec<&str> = ligne.splitn(3, '|').collect();
            if bouts.len() != 3 { continue; }
            let Some(octets) = decode_hex(bouts[2]) else { continue };
            let acc = accs.entry(bouts[1].to_string()).or_insert_with(PacketAccumulator::new);
            acc.append(&octets);
            let consomme = walk(acc.snapshot()).consumed;
            if consomme == 0 { continue; }
            let complet = acc.snapshot()[..consomme].to_vec();
            acc.discard_bytes(consomme);
            let mut paquets = Vec::new();
            aplatir(&complet, &mut paquets, 0, false);
            for p in &paquets {
                let mut i = 0usize;
                while i + 1 < p.len() {
                    if p[i] != 0x8D { i += 1; continue; }
                    let id = read_varint(p, i + 1);
                    if id.length <= 0 || !(100..=9_999_999).contains(&id.value) { i += 1; continue; }
                    let disc = i + 1 + id.length as usize;
                    if disc + 11 > p.len() { i += 1; continue; }
                    let mob = p[disc] == 0x02 && p[disc + 1] == 0x01 && p[disc + 2] == 0x00;
                    let moi = p[disc] == 0x01 && p[disc + 1] == 0x01 && p[disc + 2] == 0x01;
                    if (mob || moi) && p[disc + 7] == 0 && p[disc + 8] == 0 && p[disc + 9] == 0 && p[disc + 10] == 0 {
                        let h = disc + 3;
                        let pv = u32::from_le_bytes([p[h], p[h + 1], p[h + 2], p[h + 3]]);
                        if pv <= 100_000_000 && suivis.contains(&id.value) {
                            series.entry(id.value).or_default().push(pv as i32);
                        }
                        i = disc + 11;
                        continue;
                    }
                    i += 1;
                }
            }
        }
        for (id, serie) in series.iter() {
            let maxi = serie.iter().copied().max().unwrap_or(0);
            let mut avant_les_zeros: Vec<i32> = Vec::new();
            let mut prec: Option<i32> = None;
            for &pv in serie {
                if pv == 0 {
                    if let Some(p) = prec { if p > 0 { avant_les_zeros.push(p); } }
                }
                prec = Some(pv);
            }
            if avant_les_zeros.is_empty() { continue; }
            println!(
                "
{} ({}) : {} lectures, PV max {maxi}, {} chutes à zéro",
                pseudos.get(id).cloned().unwrap_or_default(),
                id,
                serie.len(),
                avant_les_zeros.len()
            );
            let pleins = avant_les_zeros.iter().filter(|&&v| v * 100 >= maxi * 90).count();
            let bas = avant_les_zeros.iter().filter(|&&v| v * 100 <= maxi * 25).count();
            println!("  dont {pleins} précédées de PV quasi pleins (>= 90 %), {bas} de PV bas (<= 25 %)");
            let mut apercu: Vec<String> = avant_les_zeros.iter().take(14)
                .map(|v| format!("{} ({:.0} %)", v, 100.0 * *v as f64 / maxi as f64)).collect();
            if avant_les_zeros.len() > 14 { apercu.push("…".into()); }
            println!("  valeurs avant chaque zéro : {}", apercu.join(", "));
        }
    }

    // Et ce que le produit comptera désormais : les morts des joueurs, lues
    // sur leurs PV. Seules les entités portant un pseudo ont une ligne.
    let mut joueurs: Vec<(i32, String, u32)> = pseudos
        .iter()
        .map(|(&id, nom)| (id, nom.clone(), storage.morts(id)))
        .filter(|(_, _, n)| *n > 0)
        .collect();
    joueurs.sort_by_key(|(_, _, n)| std::cmp::Reverse(*n));
    println!("
{} joueurs comptés comme tombés au moins une fois :", joueurs.len());
    for (id, nom, n) in joueurs.iter().take(20) {
        println!("  {id:>9}  {nom:<18}  {n} mort(s)");
    }

    let avec_pseudo = lignes_mortes.iter().filter(|(_, _, n, _)| !n.is_empty()).count();
    let frappeurs = lignes_mortes.iter().filter(|(_, _, _, f)| *f).count();
    println!(
        "\n  dont {avec_pseudo} avec un pseudo connu, {frappeurs} ayant porté un coup\n  \
         (ce sont ces derniers qui ont une ligne dans l'overlay)"
    );
    println!("\n{} pseudos connus au total", pseudos.len());
}
