const needle = require('needle');
const { addonBuilder } = require('stremio-addon-sdk');
const genres = require('./static/data/genres');
const { toStremioEntryMeta, toStremioCatalogMeta } = require('./lib/metadata');
const { cacheWrapMeta, cacheWrapCatalog } = require('./lib/cache');

const MAX_SIZE = 20;
const CACHE_MAX_AGE = process.env.CACHE_MAX_AGE || 7 * 24 * 60; // 7 days

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

builder.defineCatalogHandler((args) => {
	const skip = args.extra && args.extra.skip || 0;
	const id = `${args.id}|${args.extra && args.extra.genre || 'All'}|${skip}`;
	const url = args.id === 'kitsu-anime-trending' ? 'https://kitsu.io/api/edge/trending/anime' : 'https://kitsu.io/api/edge/anime';
	const query = {};
	query['include'] = 'genres';
	query['page[limit]'] = MAX_SIZE;
	query['page[offset]'] = skip;
	query['filter[subtype]'] = 'TV,OVA,ONA,movie,special';

	if (args.extra && args.extra.search) {
		// no need to cache search results
		query['filter[text]'] = args.extra.search;
		return _getCatalogEntries(url, query);
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

	return cacheWrapCatalog(id, () => _getExtendedCatalogEntries(url, query));
});

builder.defineMetaHandler((args) => {
	if (!args.id.match(/^kitsu:\d+$/)) {
		return Promise.reject(new Error('invalid id'));
	}

	const id = parseInt(args.id.match(/\d+$/)[0]);
	const query = {};
	query['include'] = 'genres,episodes';

	return cacheWrapMeta(id, () =>_getContent(`https://kitsu.io/api/edge/anime/${id}`, query)
			.then((response) => toStremioEntryMeta(response.data, response.included))
			.then((meta) => ({ meta: meta, cacheMaxAge: CACHE_MAX_AGE })));
});

async function _getExtendedCatalogEntries(url, queryParams, extendTo = 100) {
	const nextOffset = queryParams['page[offset]'] + MAX_SIZE;
	const nextQueryParams = { ...queryParams, 'page[offset]': nextOffset };
	return _getCatalogEntries(url, queryParams)
			.then((entries) => nextOffset < extendTo
				? _getExtendedCatalogEntries(url, nextQueryParams, extendTo)
								.then((nextEntries) => entries.metas.concat(nextEntries.metas))
				: entries.metas)
			.then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }))
}

async function _getCatalogEntries(url, queryParams) {
	return _getContent(url, queryParams)
			.then((response) => response.data.map((result) => toStremioCatalogMeta(result, response.included)))
			.then((metas) => ({ metas: metas, cacheMaxAge: CACHE_MAX_AGE }))
}

async function _getContent(url, queryParams) {
	return needle('get', url, queryParams, { headers: { accept: 'application/vnd.api+json'} })
			.then((response) => {
				if (response.statusCode === 200 && response.body) {
					return JSON.parse(response.body);
				}
				throw new Error('No response from kitsu');
			});
}

module.exports = builder.getInterface();