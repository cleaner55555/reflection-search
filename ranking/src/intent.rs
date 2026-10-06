//! Intent detekcija: mapira upit na šablon kolona.
//!
//! Heuristika ključnih reči sa pragovima (korak 2.3).
//! Engleski prvo; ostali jezici se dodaju po potražnji.

/// Namena upita — bira šablon kolona.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Intent {
    /// Vesti: kolone vesti + društvene + small web.
    News,
    /// Kupovina: kolone cene + recenzije + prodavnice.
    Shopping,
    /// Tehnologija: kolone dokumentacija + kod + forum.
    Tech,
    /// Opšte: standardne kolone.
    General,
}

/// Vraća namenu upita (uvek nešto — nikad greška).
#[must_use]
pub fn detect(query: &str) -> Intent {
    let q = format!(" {} ", query.to_lowercase());
    let hits = |words: &[&str]| -> u32 { words.iter().filter(|w| q.contains(**w)).count() as u32 };
    let news = hits(&[
        " news ",
        "breaking",
        "latest",
        "headline",
        "today",
        "election",
        "war ",
        "hurricane",
        "earthquake",
    ]);
    let shopping = hits(&[
        " price ",
        "buy ",
        "cheap",
        "deal",
        "discount",
        "coupon",
        "review",
        "best ",
        " vs ",
        "comparison",
        " under $",
        " under €",
    ]) * 2;
    let tech = hits(&[
        " error ",
        "github",
        " api ",
        "docker",
        "linux",
        " code ",
        "programming",
        "compiler",
        "tutorial",
        " rust ",
        " python ",
        "javascript",
        "typescript",
    ]);
    if shopping >= 2 && shopping >= news && shopping >= tech {
        Intent::Shopping
    } else if news >= 1 && news >= tech {
        Intent::News
    } else if tech >= 1 {
        Intent::Tech
    } else {
        Intent::General
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matrix_hits_85_percent() {
        let cases = [
            ("breaking news today", Intent::News),
            ("latest headlines", Intent::News),
            ("election results", Intent::News),
            ("earthquake today", Intent::News),
            ("war update", Intent::News),
            ("hurricane path", Intent::News),
            ("morning news", Intent::News),
            ("best headphones price", Intent::Shopping),
            ("buy laptop cheap", Intent::Shopping),
            ("headphones review", Intent::Shopping),
            ("pixel vs iphone comparison", Intent::Shopping),
            ("laptop under $1000", Intent::Shopping),
            ("discount coupon deal", Intent::Shopping),
            ("cheapest flights deal", Intent::Shopping),
            ("best vacuum cleaner", Intent::Shopping),
            ("rust compiler error", Intent::Tech),
            ("docker tutorial", Intent::Tech),
            ("github api pagination", Intent::Tech),
            ("python programming guide", Intent::Tech),
            ("linux code signing", Intent::Tech),
            ("javascript async tutorial", Intent::Tech),
            ("typescript generics", Intent::Tech),
            ("how to bake bread", Intent::General),
            ("capital of peru", Intent::General),
            ("weather tomorrow", Intent::General),
            ("history of rome", Intent::General),
            ("what is photosynthesis", Intent::General),
            ("nearest pharmacy", Intent::General),
            ("translate hello", Intent::General),
            ("bus schedule downtown", Intent::General),
            ("who won the match", Intent::General),
            ("meaning of dreams", Intent::General),
            ("how tall is everest", Intent::General),
            ("symptoms of flu", Intent::General),
            ("best time to visit japan", Intent::Shopping),
            ("morning news today", Intent::News),
            ("rust vs go comparison", Intent::Shopping),
            ("api error handling", Intent::Tech),
            ("cheap hotels deal", Intent::Shopping),
            ("latest python release", Intent::News),
        ];
        let mut hits = 0;
        for (q, want) in cases {
            let got = detect(q);
            if got == want {
                hits += 1;
            } else {
                eprintln!("MISS {q:?}: got {got:?}, want {want:?}");
            }
        }
        let pct = hits * 100 / cases.len();
        assert!(pct >= 85, "samo {pct}% ({hits}/{})", cases.len());
    }
}
