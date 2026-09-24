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
