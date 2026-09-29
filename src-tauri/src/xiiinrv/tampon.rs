//! Récupérer les paquets d'avant le verrouillage du port.
//!
//! LE PROBLÈME
//!   A2Tools ne connaît pas d'avance le port du jeu : il l'apprend en voyant
//!   passer assez de paquets de combat, et tant qu'il ne le connaît pas il jette
//!   tout ce qui n'y ressemble pas. Or l'inventaire et le Combat Power arrivent
//!   précisément à cet instant-là, à l'entrée en jeu, dans la seconde qui précède
//!   le verrouillage. Ils étaient donc jetés avant d'arriver jusqu'à nous.
//!
//! DEUX ESSAIS AVANT CELUI-CI, tous deux dans le filtre d'A2Tools :
//!   1. ne laisser passer que les morceaux portant l'un de nos opcodes —
//!      insuffisant : un gros paquet est découpé par le réseau, seul le premier
//!      morceau porte l'opcode et les suivants étaient jetés ;
//!   2. laisser passer tout le flux dès qu'un de nos opcodes y apparaît — les
//!      octets se retrouvent par hasard sur d'autres connexions locales : sur une
//!      vraie session nous avons retenu les flux 55987 et 64270 alors que le jeu
//!      parlait sur 61944. On filtrait le mauvais trafic.
//!
//! CE QU'ON FAIT MAINTENANT
//!   Plus rien ne change dans le filtre d'A2Tools : il reste exactement celui
//!   d'origine. On se contente de **garder une copie** des morceaux qui passent
//!   devant nous tant que le port n'est pas verrouillé, puis, au verrouillage, de
//!   relire nous-mêmes **le flux retenu par A2Tools** — celui du jeu, sans
//!   ambiguïté. Après quoi le tampon est vidé : le chemin normal prend la suite.

use parking_lot::Mutex;
use std::sync::OnceLock;
use tracing::info;

use super::collecte::{lecture_ouverte, observer};

// Plafonds : si le port ne se verrouille jamais, ce tampon ne doit pas grossir
// indéfiniment. Ils sont posés **par sens de connexion** et non seulement au
// total : sur une machine qui passe par un relais, plusieurs connexions locales
// défilent avant le verrouillage, et un unique plafond global pourrait être
// rempli par ce bruit avant même que le flux du jeu n'apparaisse. L'entrée en
// jeu tient très largement dans un sens (quelques centaines de kilo-octets).
const PLAFOND_PAR_SENS: usize = 3 * 1024 * 1024;
const PLAFOND_TOTAL: usize = 12 * 1024 * 1024;
const SENS_SUIVIS_AU_PLUS: usize = 24;

/// Port d'origine et port de destination, **dans cet ordre**. Avant le
/// verrouillage, les deux sens de la connexion défilent ici : les mêler dans un
/// seul tampon rendrait le découpage illisible. Chaque sens a donc le sien, et
/// on ne relit que celui qui va du serveur vers le client.
type Flux = (u16, u16);

static TAMPON: OnceLock<Mutex<Vec<(Flux, Vec<u8>)>>> = OnceLock::new();

fn tampon() -> &'static Mutex<Vec<(Flux, Vec<u8>)>> {
    TAMPON.get_or_init(|| Mutex::new(Vec::new()))
}

/// Garde une copie d'un morceau vu avant le verrouillage du port.
/// Ne fait rien si le partage n'est pas activé.
pub fn mettre_de_cote(origine: u16, destination: u16, donnees: &[u8]) {
    if !lecture_ouverte() || donnees.is_empty() {
        return;
    }
    let cle = (origine, destination);
    let mut tampon = tampon().lock();
    let total: usize = tampon.iter().map(|(_, o)| o.len()).sum();
    if total + donnees.len() > PLAFOND_TOTAL {
        return;
    }
    match tampon.iter_mut().find(|(k, _)| *k == cle) {
        Some((_, octets)) => {
            if octets.len() + donnees.len() <= PLAFOND_PAR_SENS {
                octets.extend_from_slice(donnees);
            }
        }
        None => {
            if tampon.len() < SENS_SUIVIS_AU_PLUS {
                tampon.push((cle, donnees.to_vec()));
            }
        }
    }
}

/// Le port vient d'être verrouillé : `origine` est le port du jeu. On relit le
/// sens serveur → client, le seul qui circule en clair, puis on vide tout.
pub fn relire(origine: u16, destination: u16) {
    let cle = (origine, destination);
    let garde = {
        let mut tampon = tampon().lock();
        let garde = tampon
            .iter()
            .position(|(k, _)| *k == cle)
            .map(|i| tampon.swap_remove(i).1);
        tampon.clear();
        garde
    };
    let Some(octets) = garde else {
        return;
    };
    if !lecture_ouverte() {
        return;
    }
    let mut paquets = 0usize;
    parcourir(&octets, &mut |paquet| {
        paquets += 1;
        observer(paquet);
    });
    info!(
        "XIII NRV : flux {:?} verrouillé, {} octets gardés relus en {} paquets",
        cle,
        octets.len(),
        paquets
    );
}

/// Le partage vient d'être coupé : on ne garde rien.
pub fn oublier() {
    tampon().lock().clear();
}

// ---------------------------------------------------------------------------
// Découpage du flux
// ---------------------------------------------------------------------------

fn varint(d: &[u8], o: usize) -> Option<(usize, usize)> {
    let mut valeur = 0usize;
    let mut decalage = 0;
    let mut lus = 0;
    loop {
        let octet = *d.get(o + lus)?;
        lus += 1;
        valeur |= ((octet & 0x7F) as usize) << decalage;
        if octet & 0x80 == 0 {
            return Some((valeur, lus));
        }
        decalage += 7;
        if decalage >= 32 {
            return None;
        }
    }
}

/// Même découpage que `consume_stream` d'A2Tools : longueur varint (moins 3) et
/// groupes compressés `FF FF`. Deux détails qui comptent : au premier niveau un
/// groupe occupe **un octet de plus**, et on se resynchronise octet par octet
/// plutôt que d'abandonner le flux à la première anomalie — un tampon commence
/// rarement pile au début d'un paquet.
pub fn parcourir(flux: &[u8], voir: &mut impl FnMut(&[u8])) {
    parcourir_a(flux, voir, 0);
}

fn parcourir_a(flux: &[u8], voir: &mut impl FnMut(&[u8]), profondeur: u32) {
    if profondeur > 4 {
        return;
    }
    let interne = profondeur > 0;
    let mut o = 0usize;
    while o < flux.len() {
        if flux[o] == 0 {
            o += 1;
            continue;
        }
        let Some((valeur, lus)) = varint(flux, o) else {
            if interne {
                break;
            }
            o += 1;
            continue;
        };
        if valeur <= 3 {
            if interne {
                break;
            }
            o += 1;
            continue;
        }
        let taille = valeur - 3;
        if taille > 65535 || o + taille > flux.len() {
            if interne {
                break;
            }
            o += 1;
            continue;
        }

        let paquet = &flux[o..o + taille];
        let est_groupe = paquet.len() > lus + 1 && paquet[lus] == 0xFF && paquet[lus + 1] == 0xFF;

        if est_groupe {
            let supplement = if interne { 0 } else { 1 };
            let fin = (o + taille + supplement).min(flux.len());
            let charge = &flux[o + lus..fin];
            if charge.len() > 6 {
                let taille_decompressee =
                    u32::from_le_bytes([charge[2], charge[3], charge[4], charge[5]]) as usize;
                if taille_decompressee > 0 && taille_decompressee <= 1_000_000 {
                    if let Ok(decompresse) =
                        lz4_flex::block::decompress(&charge[6..], taille_decompressee)
                    {
                        parcourir_a(&decompresse, voir, profondeur + 1);
                    }
                }
            }
            o += taille + supplement;
        } else {
            voir(paquet);
            o += taille;
        }
    }
}
