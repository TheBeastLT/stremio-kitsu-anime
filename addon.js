const needle = require('needle');
const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const { cacheWrapCatalog, cacheWrapMeta } = require('./lib/cache');
const { toStremioMeta } = require('./lib/metadata');

const MAX_SIZE = 20;

const manifest = {
	id: 'community.kitsu.anime',
	version: '0.0.1',
	name: 'Kitsu Anime',
	description: 'Anime catalogs from Kitsu.io',
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

builder.defineCatalogHandler((args) => {
	console.log(args);
	const skip = args.extra && args.extra.skip || 0;
	const page = skip / MAX_SIZE;
	const id = `${args.id}|${args.extra && args.extra.genre || 'All'}|${page}`;
	const url = args.id === 'kitsu-anime-trending' ? 'https://kitsu.io/api/edge/trending/anime' : 'https://kitsu.io/api/edge/anime';
	const query = {};
	query['include'] = 'genres';
	query['page[limit]'] = MAX_SIZE;
	query['page[offset]'] = skip;
	query['filter[subtype]'] = 'TV,OVA,ONA,movie,special';
	if (args.extra && args.extra.search) {
		query['filter[text]'] = args.extra.search;
	}
	if (args.extra && args.extra.genre) {
		query['filter[genres]'] = args.extra.genre;
	}
	if (args.id === 'kitsu-anime-list') {
		query['sort'] = 'createdAt';
	} if (args.id === 'kitsu-anime-rating') {
		query['sort'] = '-average_rating';
	} else if (args.id === 'kitsu-anime-popular') {
		query['sort'] = '-user_count';
	} else if (args.id === 'kitsu-anime-trending') {
		query['limit'] = '50';
	}

	return cacheWrapCatalog(id, () => _getPagedContent(url, query)
			.then((response) => response.data.map((result) => toStremioMeta(result, response.included)))
			.then((metas) => metas
					.filter((meta) => meta.type && meta.type !== 'other')
					.map((meta) => ({
						id: meta.id,
						type: meta.type,
						name: meta.name,
						description: meta.description,
						genres: meta.genres,
						poster: meta.poster,
					})))
			.then((metas) => ({ metas: metas })))
});

builder.defineMetaHandler((args) => {
	console.log(args);
	if (!args.id.match(/^kitsu:\d+$/)) {
		return Promise.reject(new Error('invalid id'));
	}

	const id = parseInt(args.id.match(/\d+$/)[0]);
	const query = {};
	query['include'] = 'genres,episodes';

	return cacheWrapMeta(id, () =>_getPagedContent(`https://kitsu.io/api/edge/anime/${id}`, query)
			.then((response) => toStremioMeta(response.data, response.included))
			.then((meta) => ({ meta: meta })))
});

async function _getPagedContent(url, queryParams) {
	return needle('get', url, queryParams, { headers: { accept: 'application/vnd.api+json'} })
			.then((response) => {
				if (response.statusCode === 200 && response.body) {
					return JSON.parse(response.body);
				}
				throw new Error('No response from kitsu');
			});
}

module.exports = builder.getInterface();