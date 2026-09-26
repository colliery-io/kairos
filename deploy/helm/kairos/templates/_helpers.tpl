{{/*
Expand the name of the chart.
*/}}
{{- define "kairos.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
We truncate at 63 chars because some Kubernetes name fields are limited to this
(by the DNS naming spec). If release name contains chart name it will be used
as a full name.
*/}}
{{- define "kairos.fullname" -}}
{{- if .Values.fullnameOverride }}
{{- .Values.fullnameOverride | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- $name := default .Chart.Name .Values.nameOverride }}
{{- if contains $name .Release.Name }}
{{- .Release.Name | trunc 63 | trimSuffix "-" }}
{{- else }}
{{- printf "%s-%s" .Release.Name $name | trunc 63 | trimSuffix "-" }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Create chart name and version as used by the chart label.
*/}}
{{- define "kairos.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels
*/}}
{{- define "kairos.labels" -}}
helm.sh/chart: {{ include "kairos.chart" . }}
{{ include "kairos.selectorLabels" . }}
{{- if .Chart.AppVersion }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
{{- end }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels
*/}}
{{- define "kairos.selectorLabels" -}}
app.kubernetes.io/name: {{ include "kairos.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
The fully-qualified image reference. image.tag defaults to the chart's
appVersion, so the chart never runs `latest` (KAIROS-A-0013).
*/}}
{{- define "kairos.image" -}}
{{- $tag := .Values.image.tag | default .Chart.AppVersion -}}
{{- printf "%s:%s" .Values.image.repository $tag -}}
{{- end }}

{{/*
The name of the service account to use.
*/}}
{{- define "kairos.serviceAccountName" -}}
{{- if .Values.serviceAccount.create }}
{{- default (include "kairos.fullname" .) .Values.serviceAccount.name }}
{{- else }}
{{- default "default" .Values.serviceAccount.name }}
{{- end }}
{{- end }}

{{/*
The name of the Secret that holds DATABASE_URL. When database.existingSecret is
set the operator brings their own; otherwise the chart renders one.
*/}}
{{- define "kairos.databaseSecretName" -}}
{{- if include "kairos.postgresqlEnabled" . }}
{{- printf "%s-postgresql" (include "kairos.fullname" .) }}
{{- else if .Values.database.existingSecret }}
{{- .Values.database.existingSecret }}
{{- else }}
{{- printf "%s-db" (include "kairos.fullname" .) }}
{{- end }}
{{- end }}

{{/*
Whether the bundled PostgreSQL runs (KAIROS-T-0188).

TRI-STATE on purpose, and this is the important part of the design.

  unset (default)  — bundle ON, unless an external database is named
  true             — bundle ON, and naming an external database is an error
  false            — bundle OFF, exactly the pre-bundle chart

The first row is what makes `helm upgrade` safe for every release that already
exists. Those all set `database.url`, and a plain `enabled: true` default would
have failed their next upgrade — or worse, quietly stood a second database up
beside the real one. Treating a named database as evidence that the bundle is not
wanted is what the operator meant, and it needs no edit from them.

Explicit `true` alongside an external database is still refused, because that is
someone asking for two databases rather than inheriting a default.
*/}}
{{- define "kairos.postgresqlEnabled" -}}
{{- if kindIs "bool" .Values.postgresql.enabled -}}
{{- if .Values.postgresql.enabled -}}true{{- end -}}
{{- else -}}
{{- if not (or .Values.database.url .Values.database.existingSecret) -}}true{{- end -}}
{{- end -}}
{{- end }}

{{/*
The bundled PostgreSQL's object name and in-cluster hostname (KAIROS-T-0188).
Kept as helpers so the StatefulSet, its Service and the DATABASE_URL that points
at it cannot drift apart.
*/}}
{{- define "kairos.postgresqlName" -}}
{{- printf "%s-postgresql" (include "kairos.fullname" .) }}
{{- end }}

{{/*
The DATABASE_URL for the bundled instance. `sslmode=disable` because both ends
are inside the cluster and the bundle is for evaluation; an operator who wants
TLS to their database wants a managed one, which is the disabled path.
*/}}
{{- define "kairos.bundledDatabaseUrl" -}}
{{- $a := .Values.postgresql.auth -}}
{{- printf "postgres://%s:%s@%s:5432/%s?sslmode=disable" $a.username $a.password (include "kairos.postgresqlName" .) $a.database }}
{{- end }}

{{/*
Refuse a configuration that asks for two databases (KAIROS-T-0188).

Only when the bundle was asked for EXPLICITLY: inheriting the default alongside
an external database is not a mistake, it is the common case, and the tri-state
above resolves it silently and correctly.
*/}}
{{- define "kairos.validateDatabase" -}}
{{- if kindIs "bool" .Values.postgresql.enabled }}
{{- if and .Values.postgresql.enabled (or .Values.database.url .Values.database.existingSecret) }}
{{- fail "database: postgresql.enabled=true AND database.url/database.existingSecret is set — that is two databases. Drop postgresql.enabled to use your own (the chart works this out on its own), or clear database.* to use the bundled evaluation one." }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Whether the bundled Dex runs (KAIROS-T-0200).

TRI-STATE, for exactly the reason `kairos.postgresqlEnabled` is:

  unset (default)  — bundle ON, unless an issuer is named OR local auth is on
  true             — bundle ON, and naming an issuer is an error
  false            — bundle OFF, exactly the pre-bundle chart

The first row is what makes `helm upgrade` safe for every release that already
exists: all of them set `config.oidc.issuerUrl`, so all of them resolve to OFF
and keep the issuer they have, with no values edit.

The local-auth clause is KAIROS-T-0208. Before it, "no issuer" meant "the
operator forgot, give them a Dex", because a Kairos with no issuer could not
start. Now `config.localAuth.enabled` with no issuer is a DELIBERATE and
supported deployment — password accounts and nothing else — and an operator who
asks for that must not silently receive an identity provider they did not ask
for, complete with a static password in their values file. Asking for both is
still allowed; it just has to be asked for.
*/}}
{{- define "kairos.dexEnabled" -}}
{{- if kindIs "bool" .Values.dex.enabled -}}
{{- if .Values.dex.enabled -}}true{{- end -}}
{{- else -}}
{{- if and (not .Values.config.oidc.issuerUrl) (not .Values.config.localAuth.enabled) -}}true{{- end -}}
{{- end -}}
{{- end }}

{{/*
An optional setting's value, or "" when the operator left it alone
(KAIROS-T-0202).

`with` cannot do this job alone: it treats 0 as unset, and one of these settings
uses 0 as a MEANINGFUL value — KAIROS_AUTH_MAX_FAILURES=0 is how an operator
turns throttling off. A `{{ with .Values...maxFailures }}` would have quietly
dropped it and left the throttle running, which is the worst way for an escape
hatch to fail: the operator did the documented thing and nothing happened.

Returns a string, so `{{- with include "kairos.setValue" ... }}` is truthy for
"0" and falsy for "".
*/}}
{{- define "kairos.setValue" -}}
{{- if kindIs "string" . -}}
{{- . -}}
{{- else if not (kindIs "invalid" .) -}}
{{- . -}}
{{- end -}}
{{- end }}

{{/*
Whether the server should trust X-Forwarded-For (KAIROS-T-0202).

Tri-state, like `kairos.dexEnabled`, because the right answer depends on the
topology and the topology is already described in these values:

  unset (default) — FOLLOW ingress.enabled
  true / false    — as you say

Following the ingress is not a guess. Behind the chart's Ingress the socket peer
IS the ingress controller, so counting it would pool every client on earth into a
single bucket, and one clumsy client would lock out everybody. With no Ingress
there is nothing in front of the pod, the header is caller-supplied, and trusting
it would hand an attacker a fresh identity per request — which does not weaken a
source-based throttle so much as delete it.

A gateway of your own is the case the override exists for: the chart cannot see
it, so you say so.
*/}}
{{- define "kairos.trustedProxy" -}}
{{- $tp := .Values.config.auth.trustedProxy -}}
{{- if kindIs "bool" $tp -}}
{{- $tp -}}
{{- else if and (kindIs "string" $tp) (ne $tp "") -}}
{{- $tp -}}
{{- else -}}
{{- .Values.ingress.enabled -}}
{{- end -}}
{{- end }}

{{- define "kairos.dexName" -}}
{{- printf "%s-dex" (include "kairos.fullname" .) }}
{{- end }}

{{/*
The bundled Dex's issuer URL (KAIROS-T-0200).

This is the hard part of the whole task, and it is why a bundled Dex NEEDS an
ingress where a bundled PostgreSQL does not.

An OIDC issuer URL is not merely an address to fetch from — it is an identity.
Dex stamps it into every token's `iss`, and the server rejects a token whose
`iss` is not the issuer it was configured with. So the URL has to be the SAME
string in two places that see the cluster differently: the browser being
redirected to log in, and the server validating the token afterwards.

An in-cluster Service DNS name (`http://release-dex:5556/dex`) is not resolvable
from a browser, so it cannot be the issuer. The answer is to serve Dex through
the same ingress as Kairos, under `/dex`, and use that public URL for both. The
server reaches it by the same name the browser does, which is a hairpin through
the ingress — acceptable, and the reason this is evaluation-only.
*/}}
{{/*
The deployment's browser-facing base URL, derived from the ingress the same way
the Ingress template derives its hosts (KAIROS-T-0200).

`https` when `ingress.tls.enabled`, `http` otherwise — a guess, but the only one
available from values, and a wrong scheme here shows up immediately as a failed
redirect rather than silently.

Note `ingress.tls` is a MAP with an `enabled` key, not a list of TLS blocks. An
earlier draft of this helper used `gt (len .Values.ingress.tls) 0`, which counts
map keys and is therefore always true — every deployment would have been told it
was on https. Read the value, not its size.
*/}}
{{- define "kairos.publicBaseUrl" -}}
{{- $scheme := ternary "https" "http" .Values.ingress.tls.enabled -}}
{{- $host := "" -}}
{{- if eq .Values.ingress.tenancyMode "single" -}}
{{- $host = .Values.ingress.host -}}
{{- else -}}
{{- $host = .Values.config.tenancy.baseDomain -}}
{{- end -}}
{{- printf "%s://%s" $scheme $host -}}
{{- end }}

{{- define "kairos.dexIssuerUrl" -}}
{{- printf "%s/dex" (include "kairos.publicBaseUrl" .) -}}
{{- end }}

{{/*
Refuse the configurations that cannot mean anything (KAIROS-T-0200,
KAIROS-T-0208).
*/}}
{{- define "kairos.validateDex" -}}
{{- /*
The contradiction is checked FIRST, deliberately. An operator who set
dex.enabled=true alongside an issuer has asked for two issuers, and telling them
to enable an ingress instead would send them to fix the wrong thing.
*/}}
{{- if kindIs "bool" .Values.dex.enabled }}
{{- if and .Values.dex.enabled .Values.config.oidc.issuerUrl }}
{{- fail "dex: dex.enabled=true AND config.oidc.issuerUrl is set — that is two issuers. Drop dex.enabled to use your own (the chart works this out on its own), or clear config.oidc.issuerUrl to use the bundled evaluation one." }}
{{- end }}
{{- end }}
{{- if include "kairos.dexEnabled" . }}
{{- if not .Values.ingress.enabled }}
{{- fail "dex: the bundled Dex needs ingress.enabled=true. An OIDC issuer URL is an identity stamped into every token, so it must be the same string for the browser and for the server — an in-cluster Service name is not reachable from a browser. Three ways forward: set ingress.enabled and a host; bring your own issuer with config.oidc.issuerUrl; or set config.localAuth.enabled=true for password accounts, which needs no issuer and therefore no ingress (KAIROS-T-0208)." }}
{{- end }}
{{- if not .Values.dex.adminPasswordHash }}
{{- fail "dex: set dex.adminEmail and dex.adminPasswordHash. The chart deliberately ships NO default password — a default would be a known credential in every install of Kairos. Generate one with:\n  docker run --rm httpd:2.4 htpasswd -bnBC 10 \"\" 'your-password' | tr -d ':\\n'" }}
{{- end }}
{{- end }}
{{- /*
KAIROS-T-0208: with no Dex, no issuer and no local auth there is no way for a
person to log in at all. The server refuses to start in that state, so without
this the operator would learn it from a CrashLoopBackOff instead of from
`helm install` — the same failure, discovered several minutes later and with the
cause several layers away.
*/}}
{{- if not (include "kairos.dexEnabled" .) }}
{{- if and (not .Values.config.oidc.issuerUrl) (not .Values.config.localAuth.enabled) }}
{{- fail "no way to log in: dex.enabled=false with neither config.oidc.issuerUrl nor config.localAuth.enabled. Pick one — bring your own issuer (KAIROS-A-0016), enable config.localAuth.enabled for password accounts, or drop dex.enabled to get the bundled evaluation Dex." }}
{{- end }}
{{- end }}
{{- end }}

{{/*
Whether the remote embedding API key is configured at all (KAIROS-T-0188, moved
from KAIROS-T-0189). A local Ollama needs none, so absence is normal.
*/}}
{{- define "kairos.embedSecretEnabled" -}}
{{- if or .Values.embeddings.apiKey .Values.embeddings.existingSecret -}}
true
{{- end -}}
{{- end }}

{{- define "kairos.embedSecretName" -}}
{{- if .Values.embeddings.existingSecret }}
{{- .Values.embeddings.existingSecret }}
{{- else }}
{{- printf "%s-embed" (include "kairos.fullname" .) }}
{{- end }}
{{- end }}

{{- define "kairos.embedSecretKey" -}}
{{- if .Values.embeddings.existingSecret }}
{{- .Values.embeddings.existingSecretKey | default "KAIROS_EMBED_API_KEY" }}
{{- else }}
{{- "KAIROS_EMBED_API_KEY" }}
{{- end }}
{{- end }}

{{/*
The key within the database Secret that holds the connection URL.
*/}}
{{- define "kairos.databaseSecretKey" -}}
{{- if .Values.database.existingSecret }}
{{- .Values.database.existingSecretKey | default "DATABASE_URL" }}
{{- else }}
{{- "DATABASE_URL" }}
{{- end }}
{{- end }}

{{/*
The Secret holding KAIROS_WEB_CLIENT_SECRET (KAIROS-T-0056). An existing Secret
when webClientSecretExistingSecret is set, else the chart-rendered one.
*/}}
{{- define "kairos.webClientSecretName" -}}
{{- if .Values.config.webClientSecretExistingSecret }}
{{- .Values.config.webClientSecretExistingSecret }}
{{- else }}
{{- printf "%s-webauth" (include "kairos.fullname" .) }}
{{- end }}
{{- end }}

{{/*
The key within the web-client-secret Secret.
*/}}
{{- define "kairos.webClientSecretKey" -}}
{{- if .Values.config.webClientSecretExistingSecret }}
{{- .Values.config.webClientSecretExistingSecretKey | default "KAIROS_WEB_CLIENT_SECRET" }}
{{- else }}
{{- "KAIROS_WEB_CLIENT_SECRET" }}
{{- end }}
{{- end }}

{{/*
Is a web client secret configured at all (inline OR existing)? (KAIROS-T-0056)
*/}}
{{- define "kairos.webClientSecretEnabled" -}}
{{- if or .Values.config.webClientSecret .Values.config.webClientSecretExistingSecret -}}
true
{{- end -}}
{{- end }}

{{/*
Tenancy guard (KAIROS-A-0005 §2 / KAIROS-A-0013): EXACTLY ONE of
config.tenancy.baseDomain or config.tenancy.singleTenant must be set. Rendering
fails loudly otherwise — the server would fail fast at boot anyway, and a
misconfigured Ingress host is worse to debug than a template error.
*/}}
{{- define "kairos.validateTenancy" -}}
{{- $base := .Values.config.tenancy.baseDomain | default "" -}}
{{- $single := .Values.config.tenancy.singleTenant | default "" -}}
{{- if and $base $single -}}
{{- fail "config.tenancy: set EXACTLY ONE of baseDomain (wildcard subdomain tenancy) or singleTenant (single-tenant mode) — both are set." -}}
{{- end -}}
{{- if and (not $base) (not $single) -}}
{{- fail "config.tenancy: set EXACTLY ONE of baseDomain (wildcard subdomain tenancy) or singleTenant (single-tenant mode) — neither is set." -}}
{{- end -}}
{{- end }}
