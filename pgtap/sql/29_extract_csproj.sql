
BEGIN;

CREATE EXTENSION IF NOT EXISTS pgtap;
CREATE EXTENSION IF NOT EXISTS code_moniker;

SELECT plan(8);

SELECT has_function('extract_csproj'::name, ARRAY['moniker', 'text'],
	'extract_csproj(moniker, text) is exposed');

WITH parsed AS (
	SELECT * FROM extract_csproj('code+moniker://app'::moniker, $t$<Project Sdk="Microsoft.NET.Sdk">
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


CREATE TEMP TABLE proj(package_moniker moniker, name text, version text);
INSERT INTO proj
	SELECT package_moniker, name, version
	FROM extract_csproj('code+moniker://app'::moniker, $t$<Project>
  <ItemGroup>
    <PackageReference Include="Acme" Version="1.0.0" />
  </ItemGroup>
</Project>
$t$);

WITH g AS (
	SELECT extract_csharp(
		'F.cs',
		E'using Acme;\n',
		'code+moniker://app'::moniker
	) AS g
), ref_targets AS (
	SELECT t AS target
	FROM g, LATERAL unnest(graph_ref_targets(g)) t
)
SELECT
	ok((SELECT count(*)::int
		FROM ref_targets r
		JOIN proj p ON p.package_moniker @> r.target) > 0,
		'package_moniker built from csproj binds extractor ref targets via @>') AS r6;


SELECT
	ok((SELECT count(*)::int FROM extract_csproj('code+moniker://app'::moniker, $t$<Project></Project>$t$)) = 0,
		'empty project yields no deps') AS r7;


SELECT * FROM finish();

ROLLBACK;
