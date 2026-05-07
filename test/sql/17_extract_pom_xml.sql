-- Maven manifest extraction.

BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(6);

SELECT has_function('extract_pom_xml'::name, ARRAY['text'],
	'extract_pom_xml(text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_pom_xml($x$
<project>
	<groupId>com.example</groupId>
	<artifactId>demo</artifactId>
	<version>0.1.0</version>
	<dependencies>
		<dependency>
			<groupId>com.google.guava</groupId>
			<artifactId>guava</artifactId>
			<version>33.0.0-jre</version>
		</dependency>
		<dependency>
			<groupId>junit</groupId>
			<artifactId>junit</artifactId>
			<version>4.13.2</version>
			<scope>test</scope>
		</dependency>
	</dependencies>
</project>
$x$)
)
SELECT
	is((SELECT version FROM parsed WHERE dep_kind = 'package'),
		'0.1.0',
		'package row carries name + version + dep_kind=package') AS r1,
	is((SELECT name FROM parsed WHERE dep_kind = 'package'),
		'com.example:demo',
		'package name = groupId:artifactId') AS r2,
	is((SELECT dep_kind FROM parsed WHERE name = 'com.google.guava:guava'),
		'compile',
		'<scope> absent defaults to compile') AS r3,
	is((SELECT dep_kind FROM parsed WHERE name = 'junit:junit'),
		'test',
		'<scope>test</scope> tagged dep_kind=test') AS r4,
	is((SELECT import_root FROM parsed WHERE name = 'com.google.guava:guava'),
		'com.google.guava:guava',
		'import_root = groupId:artifactId for join with external_pkg_root') AS r5;

-- Linkage demo: a refs subset matched to a pkg table populated from
-- pom.xml.
CREATE TEMP TABLE pkg(project moniker, name text, version text);
INSERT INTO pkg
	SELECT 'esac+moniker://app'::moniker, name, version
	FROM extract_pom_xml($x$
<project>
	<groupId>com.example</groupId>
	<artifactId>demo</artifactId>
	<version>0.1.0</version>
	<dependencies>
		<dependency>
			<groupId>com.google.guava</groupId>
			<artifactId>guava</artifactId>
			<version>33.0.0-jre</version>
		</dependency>
	</dependencies>
</project>
$x$);

SELECT is(
	(SELECT count(*)::int FROM pkg WHERE name = 'com.google.guava:guava'),
	1,
	'pom-derived pkg table joins on coordinate name') AS r6;

SELECT * FROM finish();

ROLLBACK;
