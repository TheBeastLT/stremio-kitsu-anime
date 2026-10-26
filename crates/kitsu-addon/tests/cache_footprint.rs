use std::{
    convert::Infallible,
    sync::Arc,
    time::{Duration, Instant},
};

use kitsu_addon::infrastructure::cache::{Cache, CacheSettings, NoStats, ZSTD_LEVEL};
use serde_json::{Value, json};

const WORDS: &[&str] = &[
    "the",
    "a",
    "of",
    "and",
    "to",
    "in",
    "is",
    "his",
    "her",
    "they",
    "with",
    "for",
    "on",
    "at",
    "from",
    "by",
    "as",
    "after",
    "before",
    "while",
    "when",
    "but",
    "their",
    "new",
    "old",
    "last",
    "first",
    "battle",
    "village",
    "school",
    "friend",
    "rival",
    "secret",
    "power",
    "journey",
    "island",
    "pirate",
    "captain",
    "crew",
    "ship",
    "sword",
    "dream",
    "promise",
    "festival",
    "storm",
    "night",
    "morning",
    "king",
    "queen",
    "marine",
    "admiral",
    "fruit",
    "devil",
    "ninja",
    "clan",
    "mission",
    "teacher",
    "student",
    "tournament",
    "final",
    "match",
    "truth",
    "past",
    "memory",
    "enemy",
    "ally",
    "decides",
    "reveals",
    "discovers",
    "fights",
    "escapes",
    "returns",
    "meets",
    "loses",
    "wins",
    "protects",
    "searches",
    "finds",
    "challenges",
    "betrays",
    "saves",
    "joins",
    "leaves",
    "Luffy",
    "Zoro",
    "Nami",
    "Usopp",
    "Sanji",
    "Chopper",
    "Robin",
    "Franky",
    "Brook",
    "Naruto",
    "Sasuke",
    "Sakura",
    "Kakashi",
    "Itachi",
    "Goku",
    "Vegeta",
    "Gohan",
    "Ichigo",
    "Rukia",
    "Eren",
    "Mikasa",
    "Armin",
    "Levi",
    "Tanjiro",
    "Nezuko",
    "Zenitsu",
    "Inosuke",
    "strongest",
    "hidden",
    "forbidden",
    "ancient",
    "legendary",
    "mysterious",
    "dark",
    "bright",
    "great",
    "small",
    "long",
    "short",
    "heavy",
];

struct Random(u64);

impl Random {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }

    fn below(&mut self, bound: usize) -> usize {
        (self.next() % bound as u64) as usize
    }

    fn sentence(&mut self, words: usize) -> String {
        let mut text: Vec<&str> = (0..words).map(|_| WORDS[self.below(WORDS.len())]).collect();
        let first = text[0].to_owned();
        let capitalized = first[..1].to_uppercase() + &first[1..];
        text[0] = &capitalized;
        format!("{}.", text.join(" "))
    }
}

fn meta(kitsu_id: u64, target_bytes: usize) -> Value {
    let mut random = Random(kitsu_id.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
    let description: String = (0..8).map(|_| random.sentence(18)).collect::<Vec<_>>().join(" ");
    let mut videos = Vec::new();
    let mut meta = json!({});
    for episode in 1.. {
        let released = 946_684_800_000_u64 + episode * 7 * 86_400_000;
        let days = released / 86_400_000;
        let (year, rest) = (1970 + days / 365, days % 365);
        let title_words = 4 + random.below(4);
        let sentences = 2 + random.below(3);
        videos.push(json!({
            "id": format!("kitsu:{kitsu_id}:{episode}"),
            "title": random.sentence(title_words).trim_end_matches('.').to_owned(),
            "released": format!("{year:04}-{:02}-{:02}T{:02}:00:00.000Z", 1 + rest / 31 % 12, 1 + rest % 28, random.below(24)),
            "season": 1,
            "episode": episode,
            "thumbnail": format!("https://media.kitsu.app/episodes/thumbnails/{}/original.jpg?{}", 100_000 + random.below(900_000), 1_500_000_000 + random.below(100_000_000)),
            "overview": (0..sentences).map(|_| { let words = 10 + random.below(14); random.sentence(words) }).collect::<Vec<_>>().join(" "),
        }));
        if episode % 16 == 0 {
            meta = json!({
                "meta": {
                    "id": format!("kitsu:{kitsu_id}"),
                    "kitsu_id": kitsu_id,
                    "type": "series",
                    "animeType": "TV",
                    "name": random.sentence(3).trim_end_matches('.'),
                    "slug": format!("anime-{kitsu_id}"),
                    "aliases": [random.sentence(3), random.sentence(4)],
                    "genres": ["Action", "Adventure", "Comedy", "Fantasy"],
                    "logo": format!("https://assets.fanart.tv/fanart/tv/{kitsu_id}/hdtvlogo/logo.png"),
                    "poster": format!("https://media.kitsu.app/anime/poster_images/{kitsu_id}/medium.jpg"),
                    "background": format!("https://media.kitsu.app/anime/cover_images/{kitsu_id}/original.png"),
                    "description": description,
                    "releaseInfo": "1999-",
                    "year": "1999",
                    "imdbRating": "8.3",
                    "userCount": 150_000 + kitsu_id,
                    "status": "current",
                    "runtime": "24 min",
                    "trailers": [{ "source": "S8_YwFLCh4U", "type": "Trailer" }],
                    "videos": videos,
                    "links": [{ "name": "8.3", "category": "imdb", "url": "https://imdb.com/title/tt0388629" }],
                    "imdb_id": "tt0388629",
                },
                "cacheMaxAge": 43_200,
            });
            if serde_json::to_vec(&meta).unwrap().len() >= target_bytes {
                break;
            }
        }
    }
    meta
}

fn resident_kb() -> u64 {
    let output =
        std::process::Command::new("ps").args(["-o", "rss=", "-p", &std::process::id().to_string()]).output().unwrap();
    String::from_utf8_lossy(&output.stdout).trim().parse().unwrap()
}

#[test]
#[ignore = "measurement, run with --release --ignored --nocapture"]
fn zstd_levels_on_a_100kb_meta() {
    let json = serde_json::to_vec(&meta(12, 100_000)).unwrap();
    println!("json {} bytes", json.len());
    for level in [1, 3, 6, 9, 19] {
        let rounds = 200;
        let started = Instant::now();
        let mut packed = Vec::new();
        for _ in 0..rounds {
            packed = zstd::encode_all(json.as_slice(), level).unwrap();
        }
        let compress = started.elapsed() / rounds;
        let started = Instant::now();
        for _ in 0..rounds {
            assert_eq!(zstd::decode_all(packed.as_slice()).unwrap().len(), json.len());
        }
        let decompress = started.elapsed() / rounds;
        let ratio = json.len() as f64 / packed.len() as f64;
        println!(
            "level {level:>2}: {:>6} bytes  ratio {ratio:.2}  compress {compress:?}  decompress {decompress:?}",
            packed.len()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "measurement, run with --release --ignored --nocapture"]
async fn resident_size_by_entries() {
    let settings = CacheSettings {
        enabled: true,
        budget_bytes: std::env::var("MEMORY_CACHE_MB").ok().and_then(|mb| mb.parse().ok()).unwrap_or(384) * 1024 * 1024,
        meta_ttl: Duration::from_secs(86_400),
        catalog_ttl: Duration::from_secs(86_400),
        cache_max_age: 43_200,
    };
    let cache: Cache<Infallible> = Cache::new(settings, None, Arc::new(NoStats));
    let templates: Vec<Value> = (0..64).map(|seed| meta(1_000 + seed, 100_000)).collect();
    let packed: Vec<usize> = templates
        .iter()
        .map(|template| zstd::encode_all(serde_json::to_vec(template).unwrap().as_slice(), ZSTD_LEVEL).unwrap().len())
        .collect();
    let json_bytes = serde_json::to_vec(&templates[0]).unwrap().len();
    let baseline = resident_kb();
    println!("json {json_bytes} bytes per meta, packed {} bytes, baseline rss {} MB", packed[0], baseline / 1024);
    let mut packed_total = 0;
    for id in 0..50_000_usize {
        let mut value = templates[id % templates.len()].clone();
        value["meta"]["id"] = json!(format!("kitsu:{id}"));
        packed_total += packed[id % packed.len()];
        cache.wrap_meta(&id.to_string(), || async { Ok(value) }).await.unwrap();
        if [10_000, 25_000, 50_000].contains(&(id + 1)) {
            cache.run_pending_tasks().await;
            let rss = resident_kb();
            println!(
                "{:>6} inserted, {:>6} kept ({} MB weighted): rss {} MB (+{} MB over baseline), packed inserted {} MB",
                id + 1,
                cache.memory_entries(),
                cache.memory_weight() / 1024 / 1024,
                rss / 1024,
                rss.saturating_sub(baseline) / 1024,
                packed_total / 1024 / 1024,
            );
        }
    }
}
