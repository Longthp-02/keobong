# Deploy runbook

The API runs on Google Cloud Run in Singapore (`asia-southeast1`), the database on Neon (AWS Singapore), and the web app on Vercel (`sin1`), which proxies `/api/*` to Cloud Run. Secrets live only in Google Secret Manager and Vercel; never in the repo, chat or shell history.

## One-time setup
1. **Neon:** create the project `daghep` (Postgres 16, region AWS Asia Pacific Singapore). Copy the **direct** (non-pooled) connection string, which ends in `?sslmode=require`. The first migration runs `CREATE EXTENSION postgis`, which Neon supports.
2. **Google Cloud:** create the project, link billing, and add a budget alert (for example USD 5 per month, alerts at 50/90/100%).
3. Open **Cloud Shell** in the project and run:

```bash
PROJECT_ID=<your project id>
REGION=asia-southeast1
gcloud config set project "$PROJECT_ID"
gcloud services enable run.googleapis.com cloudbuild.googleapis.com \
  artifactregistry.googleapis.com secretmanager.googleapis.com

# Secrets: typed silently, never echoed or stored in shell history.
read -rsp "DATABASE_URL: " V && printf '%s' "$V" | gcloud secrets create daghep-database-url --data-file=- && unset V; echo
read -rsp "Google client secret: " V && printf '%s' "$V" | gcloud secrets create daghep-google-client-secret --data-file=- && unset V; echo

PROJECT_NUMBER=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')
RUNTIME_SA="$PROJECT_NUMBER-compute@developer.gserviceaccount.com"
for SECRET in daghep-database-url daghep-google-client-secret; do
  gcloud secrets add-iam-policy-binding "$SECRET" \
    --member="serviceAccount:$RUNTIME_SA" --role=roles/secretmanager.secretAccessor
done
```

## Deploy (first time and every release)
```bash
git clone https://github.com/Longthp-02/keobong.git 2>/dev/null || git -C keobong pull
cd keobong
gcloud run deploy daghep-api --source backend --region "$REGION" \
  --allow-unauthenticated --min-instances 0 --max-instances 2 \
  --memory 256Mi --cpu 1 --concurrency 80 \
  --set-env-vars "FRONTEND_ORIGIN=https://daghep.vn,GOOGLE_CLIENT_ID=669288809087-96o9da5roaa70da6m0grt0102ggih5oo.apps.googleusercontent.com,GOOGLE_REDIRECT_URI=https://daghep.vn/api/auth/google/callback,RUST_LOG=info" \
  --set-secrets "DATABASE_URL=daghep-database-url:latest,GOOGLE_CLIENT_SECRET=daghep-google-client-secret:latest"
```
Answer `Y` if asked to create the `cloud-run-source-deploy` repository. If the build fails with a permission error for the compute service account, grant it `roles/cloudbuild.builds.builder` and retry.

## Migrations (after each deploy that adds one)
Migrations run explicitly, never on cold start:
```bash
IMAGE=$(gcloud run services describe daghep-api --region "$REGION" \
  --format='value(spec.template.spec.containers[0].image)')
gcloud run jobs deploy daghep-migrate --image "$IMAGE" --region "$REGION" \
  --args migrate --max-retries 0 \
  --set-secrets "DATABASE_URL=daghep-database-url:latest"
gcloud run jobs execute daghep-migrate --region "$REGION" --wait
```

## Connect the web app
1. Vercel → project `keobong` → Settings → Environment Variables: set `API_BASE_URL` to the Cloud Run service URL for Production.
2. Redeploy, because the `/api` rewrite is built at build time.
3. Google Cloud console → Google Auth Platform → Audience: publish the app. Basic scopes need no verification. Until it is published, only listed test users can sign in.

## Check
- `curl https://<service-url>/health` returns `{"status":"ok"}`.
- `https://daghep.vn/api/me` returns `401` (the proxy works).
- Sign in at `https://daghep.vn/create`, create a free match, and join it from a second account.
- Check that the Vercel rewrite passes `Set-Cookie` and `Origin` through: sign-in keeps you signed in, and sign-out works (no 403).

## Rollback
`gcloud run services update-traffic daghep-api --region "$REGION" --to-revisions <previous-revision>=100`. Migrations are additive, so older revisions keep working.
