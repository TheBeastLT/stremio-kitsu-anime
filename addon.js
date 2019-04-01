const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const kitsu = require('./lib/kitsu_api');

const CACHE_MAX_AGE = process.env.CACHE_MAX_AGE || 4 * 60 *60;

const manifest = {
	id: 'community.anime.kitsu',
	version: '0.0.1',
	name: 'Anime Kitsu',
	description: 'Unofficial Kitsu.io anime catalog addon',
	logo: 'https://i.imgur.com/ANMG9VF.png',
	background: 'https://i.imgur.com/ym4n96o.png',
	resources: ['catalog', 'meta'],
	types: ['movie', 'series'],
	catalogs: [
		{
			id: 'kitsu-anime-list',
			name: 'Kitsu',
			type: 'series',
			extra: [{ name: 'search', isRequired: false }, { name: 'genre' }],
			genres: Object.values(genres)
		},
		{
			id: 'kitsu-anime-rating',
			name: 'Kitsu Highest Rated',
			type: 'series',
			extra: [{ name: 'genre' }],
			genres: Object.values(genres)
		},
		{
			id: 'kitsu-anime-popular',
			name: 'Kitsu Most popular',
			type: 'series',
			extra: [{ name: 'genre' }],
			genres: Object.values(genres)
		},
		{
			id: 'kitsu-anime-trending',
			name: 'Kitsu Trending',
			type: 'series'
		}
	],
	idPrefixes: ['kitsu']
};
const builder = new addonBuilder(manifest);

const sortValue = {
	'kitsu-anime-list': 'createdAt',
	'kitsu-anime-rating': '-average_rating',
	'kitsu-anime-popular': '-user_count'
};
builder.defineCatalogHandler((args) => {
	const skip = args.extra && args.extra.skip || 0;
	const page = skip / kitsu.MAX_SIZE;
	const id = `${args.id}|${args.extra && args.extra.genre || 'All'}|${page}`;

	if (args.extra && args.extra.search) {
		// no need to cache search results
		return kitsu.search(args.extra.search)
				.then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }));
	}

	const options = {
		offset: skip,
		genre: args.extra && args.extra.genre,
		sort: sortValue[args.id],
		trending: args.id === 'kitsu-anime-trending'
	};

	return kitsu.animeEntries(options)
			.then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }));
});

builder.defineMetaHandler((args) => {
	console.log(args);
	if (!args.id.match(/^kitsu:\d+$/)) {
		return Promise.reject(new Error('invalid id'));
	}

	const id = parseInt(args.id.match(/\d+$/)[0]);

	return kitsu.animeData(id)
			.then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE }));
});

module.exports = builder.getInterface();