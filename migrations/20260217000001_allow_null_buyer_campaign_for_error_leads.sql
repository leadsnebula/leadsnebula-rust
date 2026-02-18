-- Allow NULL buyer_id and campaign_id on leads so error/validation leads can be
-- persisted and shown in the leads report even when the instance has no campaign.
-- list_leads uses LEFT JOIN buyers/campaigns, so NULL is already handled.

ALTER TABLE leads ALTER COLUMN buyer_id DROP NOT NULL;
ALTER TABLE leads ALTER COLUMN campaign_id DROP NOT NULL;
