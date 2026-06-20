-- Content embeddings for the content-first reco engine (see RECOSYS.md §3/§4).
--
-- Each catalogue item is embedded ONCE by a static model2vec sentence model and
-- the L2-normalized vector is cached here as a little-endian f32 BLOB (dim is
-- model-dependent — 512 floats ≈ 2 KB for potion-retrieval-32M). The request hot
-- path never re-embeds — it ranks over these precomputed
-- vectors with plain dot products. `embedding_model` stamps which model produced
-- the vector, so swapping the model invalidates stale rows (re-embedded lazily by
-- the ingest/backfill pass).
--
-- Additive + nullable (backward-compatible): legacy rows carry no embedding until
-- the next ingest/backfill fills them in.
ALTER TABLE catalog_items ADD COLUMN content_embedding BLOB;
ALTER TABLE catalog_items ADD COLUMN embedding_model   TEXT;

-- The ingest pass scans for rows whose embedding is missing or stale (model swap).
CREATE INDEX catalog_items_embedding_model_idx ON catalog_items(embedding_model);
