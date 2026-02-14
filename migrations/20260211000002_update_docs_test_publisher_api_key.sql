-- Update documentation test publisher API key to pk_test_* format.
-- Publisher ID is stable and reused in docs:
--   a1b2c3d4-e5f6-4780-a123-456789abcdef
-- New API key:
--   pk_test_3abfd248dbed82ed500426a5cac2ead3cf182ace20934ed8ad7dd5592b7b7d08

UPDATE publishers
SET
    api_key_hash = encode(digest('pk_test_3abfd248dbed82ed500426a5cac2ead3cf182ace20934ed8ad7dd5592b7b7d08', 'sha256'), 'hex'),
    api_key_prefix = 'pk_test_',
    updated_at = NOW()
WHERE id = 'a1b2c3d4-e5f6-4780-a123-456789abcdef';
