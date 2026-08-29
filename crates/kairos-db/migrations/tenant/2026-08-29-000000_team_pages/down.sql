-- Reverse KAIROS-T-0082: drop the team-pages content store. All team
-- pages, history, and announcements are unrecoverable.
DROP TABLE IF EXISTS team_announcements;
DROP TABLE IF EXISTS team_page_history;
DROP TABLE IF EXISTS team_pages;
