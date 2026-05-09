
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS pg_code_moniker;

SELECT plan(8);

SELECT has_function('extract_csproj'::name, ARRAY['text'],
	'extract_csproj(text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_csproj($t$<Project Sdk="Microsoft.NET.Sdk">
  <PropertyGroup>
    <AssemblyName>MyApp</AssemblyName>
    <Version>1.2.3</Version>
  </PropertyGroup>
  <ItemGroup>
    <PackageReference Include="Newtonsoft.Json" Version="13.0.1" />
    <PackageReference Include="Serilog">
      <Version>3.0.0</Version>
    </PackageReference>
  </ItemGroup>
  <ItemGroup>
    <ProjectReference Include="..\Other\Other.csproj" />
  </ItemGroup>
</Project>
$t$)
)
SELECT
	is((SELECT version FROM parsed WHERE name = 'MyApp' AND dep_kind = 'package'),
		'1.2.3',
		'AssemblyName + Version emit dep_kind=package') AS r1,
	is((SELECT version FROM parsed WHERE name = 'Newtonsoft.Json'),
		'13.0.1',
		'PackageReference attribute Version is captured') AS r2,
	is((SELECT version FROM parsed WHERE name = 'Serilog'),
		'3.0.0',
		'PackageReference element Version is captured') AS r3,
	is((SELECT dep_kind FROM parsed WHERE name = 'Other'),
		'project',
		'ProjectReference tagged dep_kind=project') AS r4,
	is((SELECT import_root FROM parsed WHERE name = 'Newtonsoft.Json'),
		'Newtonsoft.Json',
		'package import_root preserves the namespace name') AS r5;


CREATE TEMP TABLE proj(project moniker, name text, version text);
INSERT INTO proj
	SELECT 'pcm+moniker://app'::moniker, name, version
	FROM extract_csproj($t$<Project>
  <ItemGroup>
    <PackageReference Include="Acme" Version="1.0.0" />
  </ItemGroup>
</Project>
$t$);

WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'using Acme;\n',
		'pcm+moniker://app'::moniker
	) AS g
), refs_with_root AS (
	SELECT external_pkg_root(t) AS root
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	ok((SELECT count(*)::int FROM refs_with_root r JOIN proj p ON p.name = r.root) > 0,
		'JOIN matches refs to packages declared in csproj (single-segment namespace)') AS r6;


SELECT
	ok((SELECT count(*)::int FROM extract_csproj($t$<Project></Project>$t$)) = 0,
		'empty project yields no deps') AS r7;


SELECT * FROM finish();

ROLLBACK;
