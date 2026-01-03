-- Foreign key constraints for all tables
-- Note: Some tables reference others that may not exist yet, so this migration should run last

-- Instance relationships
ALTER TABLE instances ADD CONSTRAINT fk_instances_instance_user_id 
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;

-- Publisher relationships
ALTER TABLE publishers ADD CONSTRAINT fk_publishers_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;
ALTER TABLE publishers ADD CONSTRAINT fk_publishers_instance_user_id 
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE SET NULL;

-- Buyer relationships
ALTER TABLE buyers ADD CONSTRAINT fk_buyers_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;
ALTER TABLE buyers ADD CONSTRAINT fk_buyers_instance_user_id 
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE SET NULL;
ALTER TABLE buyers ADD CONSTRAINT fk_buyers_vertical_id 
    FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE SET NULL;
ALTER TABLE buyers ADD CONSTRAINT fk_buyers_buyer_integration_id 
    FOREIGN KEY (buyer_integration_id) REFERENCES buyer_integrations(id) ON DELETE SET NULL;

-- Buyer integration relationships
ALTER TABLE buyer_integrations ADD CONSTRAINT fk_buyer_integrations_vertical_id 
    FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE CASCADE;

ALTER TABLE buyer_integration_credentials ADD CONSTRAINT fk_buyer_integration_credentials_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE;
ALTER TABLE buyer_integration_credentials ADD CONSTRAINT fk_buyer_integration_credentials_buyer_integration_id 
    FOREIGN KEY (buyer_integration_id) REFERENCES buyer_integrations(id) ON DELETE CASCADE;
ALTER TABLE buyer_integration_credentials ADD CONSTRAINT fk_buyer_integration_credentials_vertical_id 
    FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE SET NULL;

ALTER TABLE buyer_qualification_configs ADD CONSTRAINT fk_buyer_qualification_configs_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE;
ALTER TABLE buyer_qualification_configs ADD CONSTRAINT fk_buyer_qualification_configs_vertical_id 
    FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE CASCADE;
ALTER TABLE buyer_qualification_configs ADD CONSTRAINT fk_buyer_qualification_configs_buyer_integration_id 
    FOREIGN KEY (buyer_integration_id) REFERENCES buyer_integrations(id) ON DELETE SET NULL;

-- Campaign relationships
ALTER TABLE campaigns ADD CONSTRAINT fk_campaigns_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE;
ALTER TABLE campaigns ADD CONSTRAINT fk_campaigns_publisher_id 
    FOREIGN KEY (publisher_id) REFERENCES publishers(id) ON DELETE CASCADE;
ALTER TABLE campaigns ADD CONSTRAINT fk_campaigns_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;

-- Ping tree relationships
ALTER TABLE ping_trees ADD CONSTRAINT fk_ping_trees_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE CASCADE;
ALTER TABLE ping_trees ADD CONSTRAINT fk_ping_trees_publisher_id 
    FOREIGN KEY (publisher_id) REFERENCES publishers(id) ON DELETE CASCADE;

ALTER TABLE ping_tree_campaigns ADD CONSTRAINT fk_ping_tree_campaigns_ping_tree_id 
    FOREIGN KEY (ping_tree_id) REFERENCES ping_trees(id) ON DELETE CASCADE;
ALTER TABLE ping_tree_campaigns ADD CONSTRAINT fk_ping_tree_campaigns_campaign_id 
    FOREIGN KEY (campaign_id) REFERENCES campaigns(id) ON DELETE CASCADE;

-- Lead relationships
ALTER TABLE leads ADD CONSTRAINT fk_leads_publisher_id 
    FOREIGN KEY (publisher_id) REFERENCES publishers(id) ON DELETE SET NULL;
ALTER TABLE leads ADD CONSTRAINT fk_leads_campaign_id 
    FOREIGN KEY (campaign_id) REFERENCES campaigns(id) ON DELETE SET NULL;
ALTER TABLE leads ADD CONSTRAINT fk_leads_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE SET NULL;
ALTER TABLE leads ADD CONSTRAINT fk_leads_vertical_id 
    FOREIGN KEY (vertical_id) REFERENCES verticals(id) ON DELETE CASCADE;

-- Ping/Post relationships
ALTER TABLE pings ADD CONSTRAINT fk_pings_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;

ALTER TABLE posts ADD CONSTRAINT fk_posts_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;
ALTER TABLE posts ADD CONSTRAINT fk_posts_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE SET NULL;

-- Payload relationships
ALTER TABLE ping_payloads ADD CONSTRAINT fk_ping_payloads_ping_id 
    FOREIGN KEY (ping_id) REFERENCES pings(id) ON DELETE CASCADE;
ALTER TABLE ping_payloads ADD CONSTRAINT fk_ping_payloads_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;

ALTER TABLE post_payloads ADD CONSTRAINT fk_post_payloads_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;

-- Lead sales relationships
ALTER TABLE lead_sales ADD CONSTRAINT fk_lead_sales_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;
ALTER TABLE lead_sales ADD CONSTRAINT fk_lead_sales_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE;
ALTER TABLE lead_sales ADD CONSTRAINT fk_lead_sales_campaign_id 
    FOREIGN KEY (campaign_id) REFERENCES campaigns(id) ON DELETE CASCADE;

-- Pulsar decision logs relationships
ALTER TABLE pulsar_decision_logs ADD CONSTRAINT fk_pulsar_decision_logs_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;
ALTER TABLE pulsar_decision_logs ADD CONSTRAINT fk_pulsar_decision_logs_buyer_id 
    FOREIGN KEY (buyer_id) REFERENCES buyers(id) ON DELETE CASCADE;

-- Audit/Compliance relationships
ALTER TABLE audit_logs ADD CONSTRAINT fk_audit_logs_instance_id 
    FOREIGN KEY (instance_id) REFERENCES instances(id) ON DELETE SET NULL;
ALTER TABLE audit_logs ADD CONSTRAINT fk_audit_logs_instance_user_id 
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE SET NULL;

ALTER TABLE pii_access_logs ADD CONSTRAINT fk_pii_access_logs_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE SET NULL;
ALTER TABLE pii_access_logs ADD CONSTRAINT fk_pii_access_logs_user_id 
    FOREIGN KEY (user_id) REFERENCES instance_users(id) ON DELETE SET NULL;

ALTER TABLE lead_consents ADD CONSTRAINT fk_lead_consents_lead_id 
    FOREIGN KEY (lead_id) REFERENCES leads(uuid) ON DELETE CASCADE;

ALTER TABLE password_histories ADD CONSTRAINT fk_password_histories_instance_user_id 
    FOREIGN KEY (instance_user_id) REFERENCES instance_users(id) ON DELETE CASCADE;




