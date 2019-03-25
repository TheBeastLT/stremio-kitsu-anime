const { Sequelize, Op }= require('sequelize');

const POSTGRES_URI = process.env.POSTGRES_URI || 'postgres://kitsu:p4ssword@localhost:5432/kitsu';
const OPTIONS = {
 // logging: false,
  define: {
    timestamps: false,
    freezeTableName: true
  }
};

const database = new Sequelize(POSTGRES_URI, OPTIONS);

const Anime = database.define('anime', {
  id: { type: Sequelize.INTEGER, primaryKey: true},
  slug: Sequelize.STRING,
  episode_length: Sequelize.INTEGER,
  episode_count: Sequelize.INTEGER,
  synopsis: Sequelize.TEXT,
  youtube_video_id: Sequelize.STRING,
  start_date: Sequelize.DATE,
  end_date: Sequelize.DATE,
  titles: Sequelize.HSTORE,
  canonical_title: Sequelize.STRING,
  user_count: Sequelize.INTEGER,
  popularity_rank: Sequelize.INTEGER,
  average_rating: Sequelize.NUMERIC(5, 2)
});

const Episodes = database.define('episodes', {
  id: { type: Sequelize.INTEGER, primaryKey: true},
  media_id: Sequelize.INTEGER,
  number: Sequelize.INTEGER,
  season_number: Sequelize.INTEGER,
  relative_number: Sequelize.INTEGER,
  synopsis: Sequelize.TEXT,
  airdate: Sequelize.DATE,
  length: Sequelize.INTEGER,
  titles: Sequelize.HSTORE,
  canonical_title: Sequelize.STRING,
  media_type: Sequelize.STRING,
  filler: Sequelize.BOOLEAN
});

const Mappings = database.define('mappings', {
  id: { type: Sequelize.INTEGER, primaryKey: true},
  external_site: Sequelize.STRING,
  external_id: Sequelize.STRING,
  item_id: Sequelize.INTEGER,
  item_type: Sequelize.STRING,
});

const AnimeGenres = database.define('anime_genres', {
  anime_id: { type: Sequelize.INTEGER, primaryKey: true},
  genre_id: { type: Sequelize.INTEGER, primaryKey: true}
});

const Genres = database.define('genres', {
  id: { type: Sequelize.INTEGER, primaryKey: true },
  name: Sequelize.STRING,
  slug: Sequelize.STRING,
});

const AnimeStaff = database.define('anime_staff', {
  anime_id: { type: Sequelize.INTEGER, primaryKey: true },
  person_id: { type: Sequelize.INTEGER, primaryKey: true },
  role: Sequelize.STRING
});

const Producers = database.define('genres', {
  id: { type: Sequelize.INTEGER, primaryKey: true},
  name: Sequelize.STRING,
  slug: Sequelize.STRING,
});

const StremioMeta = database.define('stremio_meta', {
  id: { type: Sequelize.STRING, primaryKey: true},
  type: { type: Sequelize.STRING, defaultValue: 'series' },
  name: Sequelize.STRING,
  slug: Sequelize.STRING,
  titles: Sequelize.ARRAY(Sequelize.STRING),
  description: Sequelize.TEXT,
  runtime: Sequelize.STRING,
  releaseInfo: Sequelize.STRING,
  genres: Sequelize.ARRAY(Sequelize.STRING),
  videos: Sequelize.JSON,
  director: Sequelize.ARRAY(Sequelize.STRING),
  cast: Sequelize.ARRAY(Sequelize.STRING),
  user_count: Sequelize.INTEGER,
  popularity_rank: Sequelize.INTEGER,
  average_rating: Sequelize.NUMERIC(5, 2),
  external_ids: Sequelize.JSON,
});

class Repository {

  constructor() {
    StremioMeta.sync();
  }

  async migrate(id) {
    const anime = await Anime.findByPk(id)
        .then((result) => result.dataValues);
    const episodes = await Episodes.findAll({ where: { media_id: id }})
        .then((values) => values.map((value) => value.dataValues));
    const genres = await AnimeGenres.findAll({ where: { anime_id: id }})
        .then((values) => values.map((value) => value.dataValues.genre_id))
        .then((genreIds) => Genres.findAll({ where: { id: { [Op.in]: genreIds }}}))
        .then((values) => values.map((value) => value.dataValues.name));
    const mappings = await Mappings.findAll({ where: { item_id: id, item_type: 'Anime' }})
        .then((values) => values.map((value) => value.dataValues))
        .then((values) => values
            .filter((mapping) => mapping.external_site !== 'thetvdb')
            .reduce((obj, next) => {
              obj[next.external_site.replace(/\/.+/, '')] = next.external_id;
              return obj;
            }, {}));
    const releaseInfo = `${anime.start_date && anime.start_date.match(/^\d+/)[0]}`;
    if (anime.end_date && !anime.end_date.startsWith(releaseInfo)) {
      releaseInfo.concat(`-${anime.end_date.match(/^\d+/)[0]}`);
    }

    StremioMeta.upsert({
      id: `kitsu${anime.id}`,
      name: anime.titles[anime.canonical_title],
      slug: anime.slug,
      titles: Object.values(anime.titles),
      description: anime.synopsis,
      runtime: `${anime.episode_length / 60} min`,
      releaseInfo: releaseInfo,
      genres: genres,
      videos: episodes.map((episode) => ({
        id: `kitsu${anime.id}:${episode.season_number}:${episode.number}`,
        title: episode.titles[episode.canonical_title],
        released: episode.airdate,
        season: episode.season_number,
        episode: episode.number,
        absolute: episode.relative_number || episode.number,
      })),
      userCount: anime.user_count,
      popularityRank: anime.popularity_rank,
      averageRating: anime.average_rating,
      externalIds: mappings,
    });
  }
}

module.exports = { Repository };