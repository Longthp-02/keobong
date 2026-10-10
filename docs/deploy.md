# Deploy runbook

The pieces:
- **API:** Google Cloud Run in Singapore (`asia-southeast1`), running as its own least-privilege service account.
- **Database:** Neon (AWS Singapore).
- **Web app:** Vercel (`sin1`), which proxies `/api/*` to Cloud Run.

Secrets live only in Google Secret Manager and Vercel, never in the repo, chat or shell history.

Every Cloud Shell session starts by setting:
```bash
PROJECT_ID=<your project id>
REGION=asia-southeast1
gcloud config set project "$PROJECT_ID"
cd ~
```

## One-time setup
### 1. Neon
1. Create the project `daghep`: Postgres 16, region AWS Asia Pacific (Singapore).
2. Copy the **direct** connection string (connection pooling off).
3. Change the query string so it ends in `?sslmode=verify-full`, removing `channel_binding=require` if present. sqlx then checks Neon's certificate. With `require` it would only encrypt.
4. The first migration runs `CREATE EXTENSION postgis`, which Neon supports.

### 2. Google Cloud project
1. Create the project and link billing.
2. Add a budget alert (Billing → Budgets & alerts), for example about USD 5 per month with email alerts at 50/90/100% of actual spend. A budget only alerts; it does not stop spending.

### 3. Services, service account and secrets (Cloud Shell)
```bash
gcloud services enable run.googleapis.com cloudbuild.googleapis.com \
  artifactregistry.googleapis.com secretmanager.googleapis.com

# Builds from source run as the compute service account; it needs the builder role.
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')
gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:$PROJECT_NUMBER-compute@developer.gserviceaccount.com" \
  --role=roles/run.builder --condition=None

# The API runs as its own account that can read only its two secrets.
gcloud iam service-accounts create daghep-api --display-name="Daghep API"
API_SA="daghep-api@$PROJECT_ID.iam.gserviceaccount.com"

# Secrets: typed silently, never echoed or stored in shell history, no trailing newline.
read -rsp "DATABASE_URL: " V && printf '%s' "$V" | gcloud secrets create daghep-database-url --data-file=- && unset V; echo
read -rsp "Google client secret: " V && printf '%s' "$V" | gcloud secrets create daghep-google-client-secret --data-file=- && unset V; echo

for SECRET in daghep-database-url daghep-google-client-secret; do
  gcloud secrets add-iam-policy-binding "$SECRET" \
    --member="serviceAccount:$API_SA" --role=roles/secretmanager.secretAccessor
done
```
The Google client secret can be viewed only once, when it is created. If it was not saved, add a new secret on the client page and store that one.

To rotate a secret, add a version with `read -rsp ... | gcloud secrets versions add <name> --data-file=-`, then deploy again. `:latest` is read when an instance starts.

### 4. Google sign-in (Google Cloud console → Google Auth Platform)
- **Clients → the web client** (in the same project): the authorized JavaScript origins include `https://daghep.vn`, and the authorized redirect URIs include `https://daghep.vn/api/auth/google/callback`. Keep the localhost entries for development. The client ID goes into `GOOGLE_CLIENT_ID` below; it is not a secret.
- **Branding:**
  - Set the home page to `https://daghep.vn`, the privacy policy to `https://daghep.vn/privacy`, and the terms to `https://daghep.vn/terms`.
  - Add `daghep.vn` as an authorized domain. Google may ask to verify it in Search Console with a DNS TXT record at iNET.
- **Audience:** publish the app. Basic scopes (`openid email profile`) need no review. Until the app is published, only listed test users can sign in.

## First deploy
```bash
git clone https://github.com/Longthp-02/keobong.git && cd keobong
gcloud run deploy daghep-api --source backend --region "$REGION" \
  --service-account "daghep-api@$PROJECT_ID.iam.gserviceaccount.com" \
  --allow-unauthenticated --min-instances 0 --max-instances 2 \
  --memory 256Mi --cpu 1 --concurrency 80 \
  --set-env-vars "FRONTEND_ORIGIN=https://daghep.vn,GOOGLE_CLIENT_ID=<OAuth client id>,GOOGLE_REDIRECT_URI=https://daghep.vn/api/auth/google/callback,RUST_LOG=info" \
  --set-secrets "DATABASE_URL=daghep-database-url:latest,GOOGLE_CLIENT_SECRET=daghep-google-client-secret:latest"
```
Answer `Y` if asked to create the `cloud-run-source-deploy` repository. Then create the tables right away, using the migration steps below with the image of the revision you just deployed. The web app does not call the API until `API_BASE_URL` is set on Vercel, so nobody sees the empty database.

## Automatic releases
After CI passes on `main`, `.github/workflows/deploy.yml` releases the API if anything under `backend/` changed since the running release. The running release's commit is stored in its `commit-sha` label. The steps are the same as the manual release below: build and push the image, deploy a revision without traffic, migrate with that image, switch traffic, then check `/health`. Vercel deploys the web app at the same time, so a web change that needs a new endpoint is live within minutes of the API.

GitHub signs in to Google Cloud with Workload Identity Federation, so no Google key is stored in GitHub. Only this repository's `main` branch can sign in. The `github-deployer` account can deploy Cloud Run, act as `daghep-api` and push images. It cannot read secrets.

One-time setup (Cloud Shell):
```bash
PROJECT_ID=daghep
PROJECT_NUMBER=$(gcloud projects describe "$PROJECT_ID" --format='value(projectNumber)')
REPO=Longthp-02/keobong
DEPLOYER="github-deployer@$PROJECT_ID.iam.gserviceaccount.com"
gcloud services enable iamcredentials.googleapis.com sts.googleapis.com
gcloud iam service-accounts create github-deployer --display-name="GitHub deployer"
gcloud projects add-iam-policy-binding "$PROJECT_ID" \
  --member="serviceAccount:$DEPLOYER" --role=roles/run.developer --condition=None
gcloud iam service-accounts add-iam-policy-binding "daghep-api@$PROJECT_ID.iam.gserviceaccount.com" \
  --member="serviceAccount:$DEPLOYER" --role=roles/iam.serviceAccountUser
gcloud artifacts repositories add-iam-policy-binding cloud-run-source-deploy --location=asia-southeast1 \
  --member="serviceAccount:$DEPLOYER" --role=roles/artifactregistry.writer
gcloud iam workload-identity-pools create github --location=global --display-name="GitHub Actions"
gcloud iam workload-identity-pools providers create-oidc keobong --location=global \
  --workload-identity-pool=github --display-name="keobong main" \
  --issuer-uri=https://token.actions.githubusercontent.com \
  --attribute-mapping="google.subject=assertion.sub,attribute.repository=assertion.repository,attribute.ref=assertion.ref" \
  --attribute-condition="assertion.repository=='$REPO' && assertion.ref=='refs/heads/main'"
gcloud iam service-accounts add-iam-policy-binding "$DEPLOYER" --role=roles/iam.workloadIdentityUser \
  --member="principalSet://iam.googleapis.com/projects/$PROJECT_NUMBER/locations/global/workloadIdentityPools/github/attribute.repository/$REPO"
```
The provider path in `deploy.yml` contains the project number `669288809087`. Update it if the project ever changes.

To release again by hand, for example after a failed run: GitHub → Actions → Deploy API → Run workflow on `main`.

## Releases (manual)
Use this when GitHub Actions is unavailable. New code goes live only after its migrations have run:
```bash
cd ~/keobong && git pull
# 1. Build and deploy the new revision without sending it traffic.
gcloud run deploy daghep-api --source backend --region "$REGION" --no-traffic \
  --service-account "daghep-api@$PROJECT_ID.iam.gserviceaccount.com"
# 2. Migrate with exactly that image (by digest).
REVISION=$(gcloud run services describe daghep-api --region "$REGION" --format='value(status.latestCreatedRevisionName)')
IMAGE=$(gcloud run revisions describe "$REVISION" --region "$REGION" --format='value(status.imageDigest)')
gcloud run jobs deploy daghep-migrate --image "$IMAGE" --region "$REGION" \
  --service-account "daghep-api@$PROJECT_ID.iam.gserviceaccount.com" \
  --args migrate --max-retries 0 --set-secrets "DATABASE_URL=daghep-database-url:latest"
gcloud run jobs execute daghep-migrate --region "$REGION" --wait
# 3. Switch traffic.
gcloud run services update-traffic daghep-api --region "$REGION" --to-latest
```
Environment variables and secrets carry over from the previous revision. Migrations are additive, so the previous revision keeps working against the migrated database.

## Connect the web app (once)
1. Vercel → project `keobong` → Settings → Domains: check that `daghep.vn` is the production domain.
2. Settings → Environment Variables: set `API_BASE_URL` to the Cloud Run service URL, with no trailing path, for Production. Use the type **Config**, not Secret: the URL is public and stays readable for debugging.
3. Redeploy. The `/api` rewrite is built at build time.

## Check
- `curl https://<service-url>/health` returns `{"status":"ok"}`, and `curl https://<service-url>/api/me` returns `401`.
- `https://daghep.vn/api/me` returns `401`, which shows the proxy works.
- Sign in at `https://daghep.vn/create`. Save a payout account, create a match, and join it from a second account.
- Sign out works, with no 403. This confirms the Vercel rewrite passes `Set-Cookie` and `Origin` through.
- Optional: scan the VietQR code with a banking app (without paying) and check the name, account, amount and memo. Cancel the test match afterwards.

First production deploy done on 2026-10-10: service `daghep-api` at `https://daghep-api-669288809087.asia-southeast1.run.app`, all checks above passed.

## Rollback
```bash
gcloud run services update-traffic daghep-api --region "$REGION" --to-revisions <previous-revision>=100
```
Traffic stays pinned until the next release's step 3 (`--to-latest`).
