-- Reverse of the KAIROS-S-0004 tenant schema (runs with search_path pinned
-- to the target tenant schema, same as up.sql). Views first, then tables in
-- reverse dependency order, then the short-code sequences.

DROP VIEW entity_directory;
DROP VIEW searchable_items;

DROP TABLE board_member_capabilities;
DROP TABLE activity_log;
DROP TABLE item_history;
DROP TABLE item_relationships;
DROP TABLE adrs;
DROP TABLE documents;
DROP TABLE tasks;
DROP TABLE initiatives;
DROP TABLE strategies;
DROP TABLE item_metadata;
DROP TABLE template_metadata;
DROP TABLE metadata_enum_options;
DROP TABLE metadata_definitions;
DROP TABLE templates;
DROP TABLE board_transitions;
DROP TABLE board_columns;
DROP TABLE boards;
DROP TABLE team_delivery_streams;
DROP TABLE delivery_streams;
DROP TABLE team_members;
DROP TABLE teams;

DROP SEQUENCE seq_strategy_code;
DROP SEQUENCE seq_initiative_code;
DROP SEQUENCE seq_task_code;
DROP SEQUENCE seq_document_code;
DROP SEQUENCE seq_adr_code;
