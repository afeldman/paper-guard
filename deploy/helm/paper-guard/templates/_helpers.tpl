{{/*
Expand the name of the chart.
*/}}
{{- define "paper-guard.name" -}}
{{- default .Chart.Name .Values.nameOverride | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Create a default fully qualified app name.
*/}}
{{- define "paper-guard.fullname" -}}
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
{{- define "paper-guard.chart" -}}
{{- printf "%s-%s" .Chart.Name .Chart.Version | replace "+" "_" | trunc 63 | trimSuffix "-" }}
{{- end }}

{{/*
Common labels.
*/}}
{{- define "paper-guard.labels" -}}
helm.sh/chart: {{ include "paper-guard.chart" . }}
app.kubernetes.io/name: {{ include "paper-guard.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
app.kubernetes.io/version: {{ .Chart.AppVersion | quote }}
app.kubernetes.io/managed-by: {{ .Release.Service }}
{{- end }}

{{/*
Selector labels (pinned to the release name).
*/}}
{{- define "paper-guard.selectorLabels" -}}
app.kubernetes.io/name: {{ include "paper-guard.name" . }}
app.kubernetes.io/instance: {{ .Release.Name }}
{{- end }}

{{/*
The name of the API-key Secret referenced by the deployment. Defaults to a
sensible name when the user has not provided one.
*/}}
{{- define "paper-guard.secretName" -}}
{{- default (printf "%s-api-key" (include "paper-guard.fullname" .)) .Values.llm.apiKeySecretName }}
{{- end }}
