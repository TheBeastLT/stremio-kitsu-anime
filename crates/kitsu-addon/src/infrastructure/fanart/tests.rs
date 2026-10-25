use serde_json::json;

use super::*;
use crate::infrastructure::{
    cache::images_ttl,
    testing::{FakeHttp, Observed, memory_cache},
};

const DAY: Duration = Duration::from_secs(24 * 60 * 60);
const SHOW: &str = "https://webservice.fanart.tv/v3/tv/74796?api_key=KEY";
const MOVIE_TMDB: &str = "https://webservice.fanart.tv/v3/movies/372757?api_key=KEY";
const MOVIE_IMDB: &str = "https://webservice.fanart.tv/v3/movies/tt0275277?api_key=KEY";
const TVDB_LOOKUP: &str = "https://thetvdb.com/api/GetSeriesByRemoteID.php?imdbid=tt0388629";

fn fanart() -> (Fanart<()>, Arc<FakeHttp>, Arc<Observed>) {
    let (http, observed) = (FakeHttp::new(), Observed::new());
    (Fanart::new(http.clone(), observed.clone(), memory_cache(), "KEY".to_owned()), http, observed)
}

fn imdb(imdb_id: &str) -> MappingInfo {
    MappingInfo { imdb_id: Some(imdb_id.to_owned()), ..MappingInfo::default() }
}

fn tvdb() -> MappingInfo {
    MappingInfo { tvdb_id: Some("74796".to_owned()), ..imdb("tt14986406") }
}

fn metahub(field: &str, size: &str, imdb_id: &str) -> String {
    format!("https://images.metahub.space/{field}/{size}/{imdb_id}/img")
}

fn image(id: &str, url: &str, lang: &str) -> Value {
    json!({ "id": id, "url": url, "lang": lang, "likes": "3" })
}

fn show(fields: &Value) -> String {
    let mut body = json!({ "name": "Bleach", "thetvdb_id": "74796" });
    body.as_object_mut().unwrap().extend(fields.as_object().unwrap().clone());
    body.to_string()
}

fn tvdb_xml(ids: &[&str]) -> String {
    let series = ids
        .iter()
        .map(|id| format!("<Series>\n<seriesid>{id}</seriesid>\n<language>en</language>\n</Series>\n"))
        .collect::<Vec<_>>()
        .concat();
    format!("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<Data>\n{series}</Data>\n")
}

fn full_show() -> Value {
    json!({
        "hdtvlogo": [image("1", "http://assets.fanart.tv/hdtvlogo-ja.png", "ja"), image("2", "http://assets.fanart.tv/hdtvlogo-en.png", "en")],
        "clearlogo": [image("3", "http://assets.fanart.tv/clearlogo.png", "en")],
        "tvposter": [image("4", "http://assets.fanart.tv/poster.jpg", "en")],
        "showbackground": [image("5", "https://assets.fanart.tv/background.jpg", "")],
        "seasonthumb": [json!({ "id": "6", "url": "http://assets.fanart.tv/season.jpg", "lang": "en", "season": "2" })],
        "tvthumb": [image("7", "http://assets.fanart.tv/thumb-ja.jpg", "ja"), image("8", "http://assets.fanart.tv/thumb-00.jpg", "00")],
    })
}

#[tokio::test]
async fn no_mapping_has_no_images() {
    let (fanart, http, _) = fanart();
    assert_eq!(fanart.images("1", None).await, Images::default());
    assert_eq!(http.urls().len(), 0);
}

#[tokio::test]
async fn a_mapping_without_ids_has_no_images() {
    let (fanart, http, _) = fanart();
    let mapping = MappingInfo { fanart_logo_id: Some("1".to_owned()), ..MappingInfo::default() };
    assert_eq!(fanart.images("1", Some(&mapping)).await, Images::default());
    assert_eq!(http.urls().len(), 0);
}

#[tokio::test]
async fn show_images_pick_english_logo_and_rewrite_to_https() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    let images = fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(
        images,
        Images {
            logo: Some("https://assets.fanart.tv/hdtvlogo-en.png".to_owned()),
            logo_lang: Some("en".to_owned()),
            poster: Some("https://assets.fanart.tv/poster.jpg".to_owned()),
            background: Some("https://assets.fanart.tv/background.jpg".to_owned()),
            thumbnail: Some("https://assets.fanart.tv/thumb-00.jpg".to_owned()),
        }
    );
    assert_eq!(http.urls(), vec![SHOW]);
}

#[tokio::test]
async fn show_request_is_a_got_json_request() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    fanart.images("43078", Some(&tvdb())).await;
    let request = &http.requests()[0];
    assert_eq!(request.accept, Some("application/json"));
    assert_eq!(request.user_agent, Some("got/7.1.0 (https://github.com/sindresorhus/got)"));
    assert_eq!(request.timeout, None);
}

#[tokio::test]
async fn fanart_logo_id_selects_its_logo_whatever_the_language() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    let mapping = MappingInfo { fanart_logo_id: Some("1".to_owned()), ..tvdb() };
    let images = fanart.images("43078", Some(&mapping)).await;
    assert_eq!(images.logo.as_deref(), Some("https://assets.fanart.tv/hdtvlogo-ja.png"));
    assert_eq!(images.logo_lang.as_deref(), Some("ja"));
}

#[tokio::test]
async fn fanart_logo_id_searches_the_second_logo_list() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    let mapping = MappingInfo { fanart_logo_id: Some("3".to_owned()), ..tvdb() };
    let images = fanart.images("43078", Some(&mapping)).await;
    assert_eq!(images.logo.as_deref(), Some("https://assets.fanart.tv/clearlogo.png"));
}

#[tokio::test]
async fn an_unknown_fanart_logo_id_falls_back_to_the_preferred_language() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    let mapping = MappingInfo { fanart_logo_id: Some("999".to_owned()), ..tvdb() };
    let images = fanart.images("43078", Some(&mapping)).await;
    assert_eq!(images.logo.as_deref(), Some("https://assets.fanart.tv/hdtvlogo-en.png"));
}

#[test]
fn language_preference_takes_the_first_en_or_00_entry() {
    let list = json!([image("1", "ja", "ja"), image("2", "00", "00"), image("3", "en", "en")]);
    assert_eq!(first_url(Some(&list)).unwrap().as_deref(), Some("00"));
}

#[test]
fn an_entry_without_language_is_preferred() {
    let list = json!([image("1", "ja", "ja"), json!({ "id": "2", "url": "none" })]);
    assert_eq!(first_url(Some(&list)).unwrap().as_deref(), Some("none"));
}

#[test]
fn without_a_preferred_language_the_first_entry_is_taken() {
    let list = json!([image("1", "ja", "ja"), image("2", "fr", "fr")]);
    assert_eq!(first_url(Some(&list)).unwrap().as_deref(), Some("ja"));
}

#[test]
fn https_rewrite_replaces_the_first_http_scheme_only() {
    let list = json!([image("1", "http://a/http://b", "en")]);
    assert_eq!(first_url(Some(&list)).unwrap().as_deref(), Some("https://a/http://b"));
}

#[test]
fn a_null_entry_throws_like_node() {
    assert!(first_url(Some(&json!([null]))).is_err());
}

#[tokio::test]
async fn seasonal_thumbnails_never_win_over_the_series_thumbnail() {
    let (fanart, http, _) = fanart();
    let fields = json!({
        "seasonthumb": [json!({ "id": "6", "url": "http://season2.jpg", "lang": "en", "season": "2" })],
        "tvthumb": [image("7", "http://thumb.jpg", "en")],
    });
    http.reply(Verb::Get, SHOW, 200, &show(&fields));
    let mapping = MappingInfo { from_season: Some(2), ..tvdb() };
    let images = fanart.images("43078", Some(&mapping)).await;
    assert_eq!(images.thumbnail.as_deref(), Some("https://thumb.jpg"));
}

#[tokio::test]
async fn seasonal_thumbnails_alone_give_no_thumbnail() {
    let (fanart, http, _) = fanart();
    let fields =
        json!({ "seasonthumb": [json!({ "id": "6", "url": "http://season2.jpg", "lang": "en", "season": "1" })] });
    http.reply(Verb::Get, SHOW, 200, &show(&fields));
    let images = fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(images.thumbnail, None);
}

#[tokio::test]
async fn tvdb_id_is_looked_up_by_imdb_id() {
    let (fanart, http, observed) = fanart();
    http.reply(Verb::Get, TVDB_LOOKUP, 200, &tvdb_xml(&["81797"]));
    let show_url = "https://webservice.fanart.tv/v3/tv/81797?api_key=KEY";
    http.reply(Verb::Get, show_url, 200, &show(&full_show()));
    let images = fanart.images("12", Some(&imdb("tt0388629"))).await;
    assert_eq!(images.logo.as_deref(), Some("https://assets.fanart.tv/hdtvlogo-en.png"));
    assert_eq!(http.urls(), vec![TVDB_LOOKUP, show_url]);
    assert_eq!(http.requests()[0].accept, Some("application/xml"));
    assert_eq!(observed.outcomes(), vec![("fanart", Outcome::Ok), ("fanart", Outcome::Ok)]);
}

#[test]
fn tvdb_xml_with_one_series() {
    assert_eq!(tvdb_series_id(&tvdb_xml(&["81797"])), Ok("81797".to_owned()));
}

#[test]
fn tvdb_xml_with_several_series_takes_the_lowest_id_numerically() {
    assert_eq!(tvdb_series_id(&tvdb_xml(&["100", "25", "300", "3"])), Ok("3".to_owned()));
}

#[test]
fn tvdb_xml_without_series_has_no_id() {
    assert_eq!(
        tvdb_series_id("<?xml version=\"1.0\" encoding=\"UTF-8\" ?>\n<Data>\n</Data>"),
        Err("no id found".to_owned())
    );
}

#[test]
fn tvdb_xml_with_an_empty_root_has_no_id() {
    assert_eq!(tvdb_series_id("<Data/>"), Err("no id found".to_owned()));
}

#[test]
fn tvdb_xml_series_whose_first_child_is_not_text_sorts_last() {
    let xml = "<Data><Series><seriesid><x>1</x></seriesid></Series><Series><seriesid>9</seriesid></Series></Data>";
    assert_eq!(tvdb_series_id(xml), Ok("9".to_owned()));
}

#[test]
fn tvdb_xml_series_without_children_throws() {
    assert!(tvdb_series_id("<Data><Series/></Data>").is_err());
}

#[test]
fn tvdb_text_that_is_not_xml_throws() {
    assert!(tvdb_series_id("garbage").is_err());
}

#[test]
fn numeric_sort_keeps_non_numbers_in_place_as_ties() {
    let mut ids: Vec<String> = ["20", "abc", "10"].iter().map(|id| (*id).to_owned()).collect();
    js_numeric_sort(&mut ids);
    assert_eq!(ids, ["20", "abc", "10"]);
}

#[tokio::test]
async fn a_tvdb_lookup_without_series_falls_back_to_movie_images() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, TVDB_LOOKUP, 200, "<Data></Data>");
    let movie = "https://webservice.fanart.tv/v3/movies/tt0388629?api_key=KEY";
    http.reply(Verb::Get, movie, 200, &json!({ "movieposter": [image("1", "http://poster.jpg", "en")] }).to_string());
    let images = fanart.images("12", Some(&imdb("tt0388629"))).await;
    assert_eq!(images.poster.as_deref(), Some("https://poster.jpg"));
    assert_eq!(http.urls()[..2], [TVDB_LOOKUP.to_owned(), movie.to_owned()]);
}

#[tokio::test]
async fn a_show_without_tvdb_id_is_an_empty_series_result_not_a_movie() {
    let (fanart, http, _) = fanart();
    http.reply(
        Verb::Get,
        SHOW,
        200,
        &json!({ "name": "Bleach", "thetvdb_id": "0", "tvposter": [image("1", "p", "en")] }).to_string(),
    );
    let images = fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(images, Images::default());
    let urls = http.urls();
    assert_eq!(urls[0], SHOW);
    assert!(!urls.iter().any(|url| url.contains("/movies/")));
}

#[tokio::test]
async fn movie_images_use_tmdb_id_first() {
    let (fanart, http, observed) = fanart();
    let movie = json!({
        "hdmovielogo": [image("1", "http://logo-fr.png", "fr")],
        "movielogo": [image("2", "http://logo-en.png", "en")],
        "movieposter": [image("3", "http://poster.jpg", "en")],
        "moviebackground": [image("4", "http://background.jpg", "")],
    });
    http.reply(Verb::Get, MOVIE_TMDB, 200, &movie.to_string());
    let mapping = MappingInfo { tmdb_id: Some("372757".to_owned()), ..MappingInfo::default() };
    let images = fanart.images("11491", Some(&mapping)).await;
    assert_eq!(
        images,
        Images {
            logo: Some("https://logo-en.png".to_owned()),
            logo_lang: Some("en".to_owned()),
            poster: Some("https://poster.jpg".to_owned()),
            background: Some("https://background.jpg".to_owned()),
            thumbnail: None,
        }
    );
    let tvdb = "https://thetvdb.com/api/GetSeriesByRemoteID.php?imdbid=undefined";
    assert_eq!(http.urls(), vec![tvdb, MOVIE_TMDB]);
    assert_eq!(observed.outcomes(), vec![("fanart", Outcome::Error), ("fanart", Outcome::Ok)]);
}

#[tokio::test]
async fn movie_images_fall_back_to_imdb_id() {
    let (fanart, http, _) = fanart();
    http.reply(
        Verb::Get,
        MOVIE_IMDB,
        200,
        &json!({ "movieposter": [image("3", "http://poster.jpg", "en")] }).to_string(),
    );
    let images = fanart.images("2", Some(&imdb("tt0275277"))).await;
    assert_eq!(images.poster.as_deref(), Some("https://poster.jpg"));
    assert!(http.urls().contains(&MOVIE_IMDB.to_owned()));
}

#[tokio::test]
async fn metahub_fills_every_image_when_fanart_has_none() {
    let (fanart, http, observed) = fanart();
    http.reply(Verb::Head, &metahub("logo", "medium", "tt0275277"), 200, "");
    http.reply(Verb::Head, &metahub("background", "medium", "tt0275277"), 404, "");
    http.reply(Verb::Head, &metahub("poster", "small", "tt0275277"), 200, "");
    let images = fanart.images("2", Some(&imdb("tt0275277"))).await;
    assert_eq!(
        images,
        Images {
            logo: Some(metahub("logo", "medium", "tt0275277")),
            poster: Some(metahub("poster", "small", "tt0275277")),
            ..Images::default()
        }
    );
    let heads: Vec<String> =
        http.requests().into_iter().filter(|request| request.verb == Verb::Head).map(|request| request.url).collect();
    assert_eq!(
        heads,
        vec![
            metahub("logo", "medium", "tt0275277"),
            metahub("background", "medium", "tt0275277"),
            metahub("poster", "small", "tt0275277")
        ]
    );
    assert_eq!(observed.outcomes(), vec![("fanart", Outcome::Error), ("fanart", Outcome::Error)]);
}

#[tokio::test]
async fn metahub_probes_only_missing_images() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&json!({ "hdtvlogo": [image("1", "http://logo.png", "en")] })));
    http.reply(Verb::Head, &metahub("background", "medium", "tt14986406"), 200, "");
    let images = fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(images.logo.as_deref(), Some("https://logo.png"));
    assert_eq!(images.background, Some(metahub("background", "medium", "tt14986406")));
    assert_eq!(images.poster, None);
    let heads = http.requests().into_iter().filter(|request| request.verb == Verb::Head).count();
    assert_eq!(heads, 2);
}

#[tokio::test]
async fn metahub_server_errors_count_as_existing() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Head, &metahub("logo", "medium", "tt0275277"), 503, "");
    let images = fanart.images("2", Some(&imdb("tt0275277"))).await;
    assert_eq!(images.logo, Some(metahub("logo", "medium", "tt0275277")));
}

#[tokio::test]
async fn metahub_transport_errors_count_as_missing() {
    let (fanart, http, _) = fanart();
    http.fail(Verb::Head, &metahub("logo", "medium", "tt0275277"), "ECONNRESET");
    let images = fanart.images("2", Some(&imdb("tt0275277"))).await;
    assert_eq!(images.logo, None);
}

#[tokio::test]
async fn fanart_failure_keeps_metahub_key_order() {
    let (fanart, http, _) = fanart();
    for (field, size) in [("logo", "medium"), ("background", "medium"), ("poster", "small")] {
        http.reply(Verb::Head, &metahub(field, size, "tt0275277"), 200, "");
    }
    let value = fanart.retrieve(&imdb("tt0275277")).await;
    let keys: Vec<&String> = value.as_object().unwrap().keys().collect();
    assert_eq!(keys, ["logo", "background", "poster"]);
}

#[tokio::test]
async fn imdb_only_mappings_share_the_cache_by_imdb_id() {
    let (fanart, http, _) = fanart();
    fanart.images("12", Some(&imdb("tt0388629"))).await;
    let requests = http.urls().len();
    fanart.images("45690", Some(&imdb("tt0388629"))).await;
    assert_eq!(http.urls().len(), requests);
}

#[tokio::test]
async fn mappings_with_a_logo_id_are_cached_by_kitsu_id() {
    let (fanart, http, _) = fanart();
    let mapping = |logo: &str| MappingInfo { fanart_logo_id: Some(logo.to_owned()), ..imdb("tt0878037") };
    fanart.images("15", Some(&mapping("2841"))).await;
    let requests = http.urls().len();
    fanart.images("750", Some(&mapping("91074"))).await;
    assert_eq!(http.urls().len(), requests * 2);
    fanart.images("750", Some(&mapping("91074"))).await;
    assert_eq!(http.urls().len(), requests * 2);
}

#[tokio::test(start_paused = true)]
async fn got_retries_transport_errors_twice_within_one_observation() {
    let (fanart, http, observed) = fanart();
    http.fail(Verb::Get, SHOW, "ECONNRESET");
    http.fail(Verb::Get, SHOW, "ECONNRESET");
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    let started = tokio::time::Instant::now();
    let images = fanart.images("43078", Some(&tvdb())).await;
    assert!(images.logo.is_some());
    assert_eq!(http.urls(), vec![SHOW, SHOW, SHOW]);
    assert_eq!(observed.outcomes(), vec![("fanart", Outcome::Ok)]);
    assert_eq!(started.elapsed(), Duration::from_secs(6));
}

#[tokio::test(start_paused = true)]
async fn got_gives_up_after_two_retries() {
    let (fanart, http, observed) = fanart();
    http.fail(Verb::Get, SHOW, "ECONNRESET");
    fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(http.urls().iter().filter(|url| *url == SHOW).count(), 3);
    assert_eq!(observed.outcomes()[0], ("fanart", Outcome::Error));
}

#[tokio::test]
async fn got_does_not_retry_error_statuses() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 500, "");
    fanart.images("43078", Some(&tvdb())).await;
    assert_eq!(http.urls().iter().filter(|url| *url == SHOW).count(), 1);
}

#[tokio::test]
async fn images_ttl_is_fourteen_days_for_an_english_logo() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&full_show()));
    assert_eq!(images_ttl(&fanart.retrieve(&tvdb()).await), DAY * 14);
}

#[tokio::test]
async fn images_ttl_is_four_days_for_another_logo_language() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Get, SHOW, 200, &show(&json!({ "hdtvlogo": [image("1", "http://logo.png", "ja")] })));
    assert_eq!(images_ttl(&fanart.retrieve(&tvdb()).await), DAY * 4);
}

#[tokio::test]
async fn images_ttl_is_four_days_for_a_metahub_logo() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Head, &metahub("logo", "medium", "tt0275277"), 200, "");
    assert_eq!(images_ttl(&fanart.retrieve(&imdb("tt0275277")).await), DAY * 4);
}

#[tokio::test]
async fn images_ttl_is_two_days_without_a_logo() {
    let (fanart, http, _) = fanart();
    http.reply(Verb::Head, &metahub("poster", "small", "tt0275277"), 200, "");
    assert_eq!(images_ttl(&fanart.retrieve(&imdb("tt0275277")).await), DAY * 2);
}
