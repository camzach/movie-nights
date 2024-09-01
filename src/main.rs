use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::Html,
    routing::{get, post},
    Form, Json, Router,
};
use handlebars::Handlebars;
use regex::Regex;
use reqwest::Client;
use rust_embed::RustEmbed;
use serde::{Deserialize, Serialize, Serializer};
use sqlx::{
    postgres::{PgConnectOptions, PgPool, PgPoolOptions},
    prelude::FromRow,
    query, query_as,
    types::time::Date,
};
use time::Month;

#[derive(Serialize, FromRow)]
struct MovieProposal {
    imdb_id: String,
    #[serde(serialize_with = "serialize_date")]
    proposed_on: Date,
    proposed_by: String,
    #[serde(serialize_with = "serialize_date_option")]
    watched: Option<Date>,
    vetos: i32,
}
fn serialize_date<S>(date: &Date, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let mut result: String = "".into();
    result += &date.year().to_string();
    result += "/";
    result += match &date.month() {
        Month::January => "1",
        Month::February => "2",
        Month::March => "3",
        Month::April => "4",
        Month::May => "5",
        Month::June => "6",
        Month::July => "7",
        Month::August => "8",
        Month::September => "9",
        Month::October => "10",
        Month::November => "11",
        Month::December => "12",
    };
    result += "/";
    result += &date.day().to_string();
    serializer.serialize_str(&result)
}
fn serialize_date_option<S>(date: &Option<Date>, serializer: S) -> Result<S::Ok, S::Error>
where
    S: Serializer,
{
    let Some(date) = date else {
        return serializer.serialize_str("");
    };
    serialize_date(date, serializer)
}

#[derive(Clone)]
struct AppState<'a> {
    pool: PgPool,
    handlebars: Handlebars<'a>,
    reqwest_client: Client,
}

#[derive(RustEmbed)]
#[folder = "templates"]
#[include = "*.hbs"]
struct Assets;

#[tokio::main]
async fn main() {
    let db_url: PgConnectOptions = std::env::var("DATABASE_URL")
        .expect("no db url set")
        .parse()
        .expect("failed to parse pgconnectoptions");
    let pool = PgPoolOptions::new()
        .connect_with(db_url)
        .await
        .expect("can't connect to db");

    sqlx::migrate!().run(&pool).await.unwrap();

    let mut handlebars = Handlebars::new();
    handlebars
        .register_embed_templates::<Assets>()
        .expect("Failed to register template");

    let reqwest_client = reqwest::Client::builder().use_rustls_tls().build().unwrap();

    let app = Router::new()
        .route("/", get(index))
        .route("/movies", get(list_movies).post(add_movie))
        .route("/movies/watch", post(watch_movie))
        .route("/movies/veto", post(veto_movie))
        .with_state(AppState {
            pool,
            handlebars,
            reqwest_client,
        });

    let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await.unwrap();

    axum::serve(listener, app).await.unwrap();
}

#[derive(Deserialize)]
struct OMDbResponse {
    Title: String,
}
#[derive(Serialize)]
struct MovieListing {
    title: String,
    proposed_by: String,
    #[serde(serialize_with = "serialize_date")]
    proposed_on: Date,
    vetos: i32,
    imdb_id: String,
}

async fn index(
    State(AppState {
        pool,
        handlebars,
        reqwest_client,
    }): State<AppState<'_>>,
) -> Result<Html<String>, (StatusCode, String)> {
    let db_query = query_as!(MovieProposal, "SELECT * FROM movies WHERE watched IS NULL")
        .fetch_all(&pool)
        .await
        .expect("Failed to fetch movies");

    let listings = json_movies_from_db(db_query, &reqwest_client).await;

    Ok(handlebars
        .render("index.hbs", &listings)
        .expect("Failed to render template")
        .into())
}

async fn json_movies_from_db(
    db_response: Vec<MovieProposal>,
    client: &Client,
) -> Vec<MovieListing> {
    let api_key = std::env::var("OMDB_KEY").unwrap();
    let mut listings = vec![];
    for row in db_response {
        let request = client
            .get("https://www.omdbapi.com")
            .query(&[("apikey", &api_key), ("i", &row.imdb_id)])
            .build()
            .expect("Failed to build odmb query");
        let resposne = client
            .execute(request)
            .await
            .expect("Failed to execute omdb query")
            .json::<OMDbResponse>()
            .await
            .expect("Failed to parse omdb response to json");
        listings.push(MovieListing {
            title: resposne.Title,
            proposed_by: row.proposed_by,
            proposed_on: row.proposed_on,
            vetos: row.vetos,
            imdb_id: row.imdb_id,
        });
    }
    listings
}

#[derive(Deserialize)]
struct ListMoviesParams {
    watched: Option<bool>,
}

async fn list_movies(
    State(AppState { pool, .. }): State<AppState<'_>>,
    query: Query<ListMoviesParams>,
) -> Result<String, (StatusCode, String)> {
    let result: Result<Vec<MovieProposal>, _> = match query.watched {
        None => sqlx::query_as("SELECT * FROM movies"),
        Some(true) => sqlx::query_as("SELECT * FROM movies WHERE watched IS NOT NULL"),
        Some(false) => sqlx::query_as("SELECT * FROM movies WHERE watched IS NULL"),
    }
    .fetch_all(&pool)
    .await;

    match result {
        Ok(record_vec) => Ok(record_vec
            .iter()
            .map(|record| record.imdb_id.to_owned())
            .collect::<Vec<String>>()
            .join("\n")),
        Err(e) => Err((StatusCode::IM_A_TEAPOT, e.to_string())),
    }
}

#[derive(Deserialize)]
struct AddMovieBody {
    imdb_id: String,
    proposed_by: String,
}

async fn add_movie(
    State(AppState {
        pool,
        reqwest_client,
        ..
    }): State<AppState<'_>>,
    Form(body): Form<AddMovieBody>,
) -> Result<Json<Vec<MovieListing>>, (StatusCode, String)> {
    let parsed_id = Regex::new(r#"[a-z]{2}\d+"#)
        .unwrap()
        .find(&body.imdb_id)
        .unwrap()
        .as_str();
    sqlx::query!(
        "INSERT INTO movies (imdb_id, proposed_on, proposed_by) VALUES ($1, CURRENT_DATE, $2)",
        parsed_id,
        body.proposed_by
    )
    .execute(&pool)
    .await
    .expect("Failed to insert");
    let db_query = sqlx::query_as!(MovieProposal, "SELECT * FROM movies")
        .fetch_all(&pool)
        .await
        .expect("Failed to retrieve movies");

    let listings = json_movies_from_db(db_query, &reqwest_client).await;
    Ok(Json(listings))
}

#[derive(Deserialize)]
struct WatchMovieBody {
    imdb_id: String,
}
async fn watch_movie(
    State(AppState { pool, .. }): State<AppState<'_>>,
    Form(body): Form<WatchMovieBody>,
) -> Result<String, (StatusCode, String)> {
    query!(
        "UPDATE movies SET watched = CURRENT_DATE WHERE imdb_id = $1",
        body.imdb_id
    )
    .execute(&pool)
    .await
    .expect("Failed to update movie");
    Ok("Done".into())
}

#[derive(Deserialize)]
struct VetoMovieBody {
    imdb_id: String,
}
async fn veto_movie(
    State(AppState { pool, .. }): State<AppState<'_>>,
    Form(body): Form<VetoMovieBody>,
) -> Result<String, (StatusCode, String)> {
    query!(
        "UPDATE movies SET vetos = movies.vetos + 1 WHERE imdb_id = $1",
        body.imdb_id
    )
    .execute(&pool)
    .await
    .expect("Failed to update movie");
    Ok("Done".into())
}
