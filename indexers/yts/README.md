# YTS Indexer

A public movie torrent indexer for the YTS v2 JSON API. It supports recent, RSS, automatic, and interactive movie searches by title or IMDb id, and returns one release per quality with seeders, leechers, size, info hash, and a magnet.

## Configure in Scryer

No credentials are used. **api_url** defaults to https://movies-api.accel.li, the API host YTS keeps reachable when the front end moves. **site_url** defaults to https://yts.gg and replaces the domain YTS bakes into the download, details, and poster URLs it returns, so a proxy can be used without those links pointing back at the blocked domain. **minimum_seeders** is a host-side release-selection preference with a default of 1.

## Behavior and limits

Searches send one term: the IMDb id when Scryer has one, otherwise the title with every run of punctuation collapsed to a single space, which is what the YTS matcher answers. Fifty results are requested per search because the API returns nothing at all above that.

The API describes a release as a set of fields rather than as a name, so the plugin assembles the release title — quality, WEBRip or BRRip, 5.1 and 10Bit when present, codec, and the `-YTS` group — the same way Prowlarr's definition does, which is the form release parsers expect. Each entry in a movie's torrent list becomes its own release, the quality stands in for the provider category, and a movie with no matches is reported as an empty result rather than a fault, because the API omits the movie list entirely instead of sending an empty one. The plugin advertises a three-second rate limit and marks releases freeleech-equivalent, matching a public tracker with no ratio accounting.
