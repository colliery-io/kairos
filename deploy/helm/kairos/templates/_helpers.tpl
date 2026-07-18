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
{{- if .Values.database.existingSecret }}
{{- .Values.database.existingSecret }}
{{- else }}
{{- printf "%s-db" (include "kairos.fullname" .) }}
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
