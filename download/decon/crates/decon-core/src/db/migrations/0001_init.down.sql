-- 0001_init (down): полный откат начальной схемы (только для разработки,
-- SPEC §6: down-файлы обязательны и тестируются).

DROP TABLE IF EXISTS schema_version;
DROP TABLE IF EXISTS ffqm_results;
DROP TABLE IF EXISTS trainer_results;
DROP TABLE IF EXISTS probes;
DROP TABLE IF EXISTS responses;
DROP TABLE IF EXISTS session_events;
DROP TABLE IF EXISTS sessions;
DROP TABLE IF EXISTS scale_versions;
DROP TABLE IF EXISTS scales;
DROP TABLE IF EXISTS settings;
