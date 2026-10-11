-- Track lists only ever show live (non-missing) rows. Partial indexes that carry the same predicate
-- let a deep OFFSET page skip rows without a table lookup per row, and let COUNT(*) read a small index.
CREATE INDEX tracks_live_title ON tracks(title COLLATE NOCASE, id) WHERE missing_since IS NULL;
CREATE INDEX tracks_live_artist ON tracks(
  artist COLLATE NOCASE, album COLLATE NOCASE, COALESCE(disc_no, 1), track_no IS NULL, track_no, title COLLATE NOCASE, id
) WHERE missing_since IS NULL;
-- Superseded by tracks_live_title; keeping both lets the planner pick the one that needs a table lookup per row.
DROP INDEX tracks_title;
