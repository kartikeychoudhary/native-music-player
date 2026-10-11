-- A track appears in a playlist at most once. Older databases could hold duplicates: keep the first copy.
DELETE FROM playlist_items WHERE id NOT IN (SELECT MIN(id) FROM playlist_items GROUP BY playlist_id, track_id);
CREATE UNIQUE INDEX playlist_items_unique ON playlist_items(playlist_id, track_id);
