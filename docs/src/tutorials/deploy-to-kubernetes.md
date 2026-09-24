# Deploy Kairos to Kubernetes

In this tutorial we will run Kairos on a Kubernetes cluster from the published
chart and image, with a database and an identity provider beside it. By the end
you will have a deployment answering `/healthz` and `/readyz` and serving the
interface, and you will have seen what Kairos needs from a cluster and what it
brings itself.

It takes about twenty minutes, most of which is waiting for images to pull.

## Before we start

You will need:

- **Docker**, running.
- **[kind](https://kind.sigs.k8s.io/)** — we will create a throwaway cluster
  and delete it at the end.
- **kubectl** and **Helm 3.8 or newer** (the chart is published as an OCI
  artifact, which older Helm cannot pull).

We use kind because this is a lesson, not a production deployment. Everything
here works the same on a real cluster; the database and identity provider are
the parts you would replace.

Kairos ships neither of those on purpose — state and identity are yours. So
the first two things we install are a throwaway Postgres and a throwaway Dex,
pinned to exact versions so this lesson behaves the same every time. **Neither
is suitable for anything but a tutorial**: no persistence, no backups, one
hard-coded password.

## Create the cluster

```sh
kind create cluster --name kairos-tutorial --wait 120s
kubectl create namespace kairos
```

You should see kind report the cluster ready and `namespace/kairos created`.

## Install a throwaway database

```sh
kubectl -n kairos create deployment postgres --image=postgres:16 --port=5432
kubectl -n kairos set env deployment/postgres \
  POSTGRES_PASSWORD=kairos POSTGRES_USER=kairos POSTGRES_DB=kairos
kubectl -n kairos expose deployment postgres --port=5432
kubectl -n kairos rollout status deployment/postgres --timeout=240s
```

```
deployment "postgres" successfully rolled out
```

## Install a throwaway identity provider

Save this as `dex.yaml`:

```yaml
apiVersion: v1
kind: ConfigMap
metadata:
  name: dex
  namespace: kairos
data:
  config.yaml: |
    issuer: http://dex.kairos.svc.cluster.local:5556/dex
    storage:
      type: memory
    web:
      http: 0.0.0.0:5556
    oauth2:
      skipApprovalScreen: true
    staticClients:
      - id: kairos-web
        name: Kairos Web GUI
        public: true
        redirectURIs:
          - http://localhost:8080/callback
    enablePasswordDB: true
    staticPasswords:
      - email: admin@example.com
        # bcrypt of "admin-password"
        hash: "$2y$10$2b2cU8CPhOTaGrs1HRQuAueS7JTT5ZHsHSzYiFPm1leZck7Mc8T4W"
        username: admin
        userID: "00000000-0000-0000-0000-000000000001"
---
apiVersion: apps/v1
kind: Deployment
metadata:
  name: dex
  namespace: kairos
spec:
  replicas: 1
  selector: { matchLabels: { app: dex } }
  template:
    metadata: { labels: { app: dex } }
    spec:
      containers:
        - name: dex
          image: ghcr.io/dexidp/dex:v2.41.1
          args: ["dex", "serve", "/etc/dex/config.yaml"]
          ports: [{ containerPort: 5556 }]
          volumeMounts:
            - { name: config, mountPath: /etc/dex }
      volumes:
        - name: config
          configMap: { name: dex }
---
apiVersion: v1
kind: Service
metadata:
  name: dex
  namespace: kairos
spec:
  selector: { app: dex }
  ports: [{ port: 5556, targetPort: 5556 }]
```

Apply it and wait:

```sh
kubectl apply -f dex.yaml
kubectl -n kairos rollout status deployment/dex --timeout=240s
```

```
deployment "dex" successfully rolled out
```

## Install Kairos

Save this as `values.yaml`:

```yaml
database:
  url: "postgres://kairos:kairos@postgres.kairos.svc.cluster.local:5432/kairos"
config:
  oidc:
    issuerUrl: "http://dex.kairos.svc.cluster.local:5556/dex"
    audience: "kairos-web"
  tenancy:
    singleTenant: "demo"
```

Those three are the minimum. The chart refuses to render without the two OIDC
values, which is deliberate — a deployment that cannot say who its users are is
not a deployment.

```sh
helm install kairos oci://ghcr.io/colliery-io/charts/kairos --version 0.1.1 \
  -n kairos -f values.yaml --wait --timeout 6m
```

Helm pulls the chart from the registry, then the cluster pulls the image:

```
Pulled: ghcr.io/colliery-io/charts/kairos:0.1.1
NAME: kairos
STATUS: deployed
```

The container applies its own database migrations on boot, so there is no
separate migrate step.

## Check it is running

```sh
kubectl -n kairos get pods
```

Both Kairos replicas should be `1/1 Running`, alongside Postgres and Dex:

```
NAME                        READY   STATUS    RESTARTS   AGE
dex-6d556d5f6d-nh6ww        1/1     Running   0          3m
kairos-5785ff87b8-52d9d     1/1     Running   0          1m
kairos-5785ff87b8-7nqvk     1/1     Running   0          1m
postgres-6f59b65747-djxn8   1/1     Running   0          3m
```

## Reach it

```sh
kubectl -n kairos port-forward svc/kairos 8080:80
```

Leave that running, and in another terminal:

```sh
curl http://localhost:8080/healthz
curl http://localhost:8080/readyz
```

```
ok
ready
```

Those two answer different questions, and the difference matters when you
deploy for real. `/healthz` says the process is alive. `/readyz` says it can
reach its database — which is what Kubernetes should gate traffic on.

Now open <http://localhost:8080>. Kairos serves the interface from the same
process, and you will be sent to Dex to sign in as `admin@example.com` /
`admin-password`.

The tenant is empty: this is a fresh deployment, not the demo data from
[Run Kairos locally](run-kairos-locally.md). An empty Kairos is the honest
starting point for a real one.

## Clean up

```sh
kind delete cluster --name kairos-tutorial
```

That removes the cluster and everything in it, including the database.

## What you have seen

Kairos is one stateless image. It brought the interface, the API, MCP and SCIM
in a single process, and it needed exactly two things from you: somewhere to
keep state, and somebody to say who users are.

That is the whole deployment story, and the rest is making those two things
production-grade rather than throwaway.

## Where to go next

- [Install with Helm](../how-to/install-with-helm.md) — the values a real
  deployment sets, and what each one does
- [Configure an OIDC issuer](../how-to/configure-an-oidc-issuer.md) — replacing
  the throwaway Dex with your own
- [Provision a tenant](../how-to/provision-a-tenant.md) — this lesson pinned a
  single tenant; a real deployment may host several
- [Back up and restore](../how-to/back-up-and-restore.md) — the database is the
  only state, and that guide says what to do about it
