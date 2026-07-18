-- Revert the public schema (KAIROS-S-0004): drop in reverse dependency order.
DROP TABLE system_board_defaults;
DROP TABLE system_template_metadata;
DROP TABLE system_metadata_enum_options;
DROP TABLE system_metadata_definitions;
DROP TABLE system_templates;
DROP TABLE organization_members;
DROP TABLE users;
DROP TABLE organizations;
