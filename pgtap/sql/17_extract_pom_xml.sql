
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(7);

SELECT has_function('extract_pom_xml'::name, ARRAY['moniker', 'text'],
	'extract_pom_xml(moniker, text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_pom_xml('code+moniker://app'::moniker, $x$
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

CREATE TEMP TABLE pkg(package_moniker moniker, name text, version text);
INSERT INTO pkg
	SELECT package_moniker, name, version
	FROM extract_pom_xml('code+moniker://app'::moniker, $x$
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
	(SELECT package_moniker FROM pkg WHERE name = 'com.google.guava:guava'),
	'code+moniker://app/external_pkg:com.google.guava:guava'::moniker,
	'package_moniker built on supplied project with head=import_root') AS r6;

SELECT * FROM finish();

ROLLBACK;
