# The Pirate Bay Indexer

A public torrent indexer for The Pirate Bay's apibay JSON API. It supports recent, RSS, automatic, and interactive searches using title, season, episode, category, and limit inputs, and returns magnet releases with seeders, leechers, size, and the IMDb id apibay reports.

## Configure in Scryer

No credentials are used. **api_url** defaults to https://apibay.org and is the JSON API the plugin searches. **site_url** defaults to https://thepiratebay.org and is only used to build the details link of a release, so a mirror can be pointed at a working front end without moving the API. **top100** selects the precompiled feed answering keyword-less searches and defaults to All; the recent feed skews heavily to video, so an indexer used for audio or books should pick the matching group. **uploader** filters to a single case-sensitive uploader username when set. **minimum_seeders** is a host-side release-selection preference with a default of 1.

## Behavior and limits

Queries are lower-cased and reduced to dot-separated words before they are sent, because apostrophes and CJK ideograms make the TPB search engine return nothing at all. Season and episode inputs are appended as an `SxxExx` token. Titles are rewritten so `Season 4` becomes `S04` and a spaced ` - GROUP` suffix becomes ` -GROUP`, which is what a release parser needs to read the season and the release group.

apibay reports "nothing found" as a single row with id 0 rather than an empty array, and that row is dropped. Numbers are quoted in `q.php` answers and unquoted in the precompiled feeds, so both shapes are accepted. Releases carry a magnet built from the reported info hash and are marked freeleech-equivalent, matching a public tracker with no ratio accounting. The plugin advertises a two-second rate limit, returns at most 100 releases per search, and offers no provider-native external-ID lookup because the API accepts free text only.
