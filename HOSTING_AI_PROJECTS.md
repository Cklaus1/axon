# Hosting AI Projects Safely — Options, Comparison, and a Recommended Setup

A practical guide to where and how to run AI projects (agents, untrusted AI-generated code,
inference/training) with real isolation — and how Axon's `axon-vm` / nanovm fits.

> **The one thing to internalize first:** there are **two independent choices**, plus a
> third that skips both:
> - **(A) Isolation tech** — how you sandbox each project (container / microVM / VM / unikernel).
> - **(B) Host** — where it runs (your box / rented bare metal / VPS).
> - **(C) Managed sandbox service** — someone runs (A) and (B) for you.
>
> People conflate "VPS / microVM / nanovm / VM" — but a **VPS is a host**, while
> **microVM/VM/container are isolation tech you run on a host.** You combine them.

---

## 0. What "KVM" is (since it gates everything)

**KVM** (Kernel-based Virtual Machine) is the Linux hypervisor — a kernel module that uses
your CPU's hardware virtualization (Intel VT-x / AMD-V) and exposes `/dev/kvm`. Firecracker,
QEMU, Cloud Hypervisor, and Kata all sit on top of it to run hardware-isolated VMs at
near-native speed.

**The gotcha:** most cheap VPSes are *themselves* VMs with nested virtualization **disabled**,
so they have **no `/dev/kvm`** → you cannot run microVMs/VMs on them, only containers. Always
check first:

```bash
ls -l /dev/kvm    # missing → no microVMs/VMs on this host, containers only
```

---

## A. Isolation technology (the per-project sandbox)

| Tech | Isolation | Boot | Overhead | GPU | Runs arbitrary code? | Maturity |
|---|---|---|---|---|---|---|
| **Docker container** | Weak (shared host kernel) | instant | ~0 | ✅ nvidia runtime | ✅ anything | ⭐⭐⭐⭐⭐ |
| **gVisor** (runsc) | Medium (userspace syscall filter) | fast | 10–30% syscall cost | ⚠️ limited (nvproxy) | ✅ most | ⭐⭐⭐⭐ |
| **Kata Containers** | **Strong (hardware VM)** | sub-second | moderate | ✅ via passthrough | ✅ **runs OCI/Docker images** | ⭐⭐⭐⭐ |
| **Firecracker microVM** | **Strong (hardware VM)** | ~125 ms | low | ❌ | ✅ (Linux guest) | ⭐⭐⭐⭐⭐ (AWS Lambda/Fargate) |
| **Cloud Hypervisor** | Strong (hardware VM) | fast | low–moderate | ⚠️ some VFIO | ✅ (Linux guest) | ⭐⭐⭐⭐ |
| **Unikernel** (Nanos/OPS) | Strong + tiny attack surface | very fast | very low | ❌ | ⚠️ one app, must be compatible | ⭐⭐ niche |
| **`axon` nanovm** (this repo) | Strong + **smallest TCB** | fast | very low | ❌ | ❌ **not yet** — boots + enforces only | ⭐ roadmap |
| **Full VM** (QEMU/Proxmox) | Strongest mature | seconds | high | ✅ full passthrough (VFIO) | ✅ anything | ⭐⭐⭐⭐⭐ |

**Takeaways**
- For **untrusted AI code / agents**: **Firecracker** or **Kata** — hardware isolation at
  near-container speed. Kata is much easier (it runs normal Docker images); Firecracker is
  leaner but you build/manage guest images (or use `firecracker-containerd` / Kata-on-Firecracker).
- **Containers alone are not an isolation boundary for untrusted code** — they share the host
  kernel. (This is exactly what the flagship Docker+seccomp foil demonstrates: seccomp blocks
  ~1 of 3 escapes; the kernel is shared.)
- **GPU + strong isolation is genuinely hard** — see §D.

---

## B. The host (where you run the isolation tech)

| Host | `/dev/kvm`? | GPU | Cost (ballpark, verify) | Ops burden |
|---|---|---|---|---|
| **Your own box / homelab** | ✅ full | if you buy it | capex only | high |
| **Rented bare metal** — Hetzner, OVH, Vultr Bare Metal, Latitude.sh, Equinix Metal | ✅ full | optional (pricey) | Hetzner *auction* ~€35–80/mo; GPU boxes $$$ | medium–high |
| **Nested-virt VPS** — GCP nested-virt, AWS `.metal`, some Hetzner Cloud | ✅ works | varies | higher $/core | medium |
| **Standard budget VPS** — most DigitalOcean/Linode/Vultr cloud | ❌ **no KVM** | ❌ | cheap | low — *but containers only* |

**The cheapest reliable "real isolation" host is a Hetzner auction dedicated server** (full
`/dev/kvm`, ~€35–50/mo). DigitalOcean/Linode standard droplets generally **cannot** run microVMs.

---

## C. Managed sandbox services (skip running a host)

If the goal is "a place to run AI projects safely" with **zero ops**, these run the microVM
isolation for you — several are purpose-built for AI:

| Service | What it is | Best for |
|---|---|---|
| **E2B** | Firecracker sandboxes built for **AI agent code execution** | agents running untrusted code |
| **Modal** | Serverless containers + **GPUs** for AI, fast cold start | GPU inference/training |
| **Fly.io Machines** | On-demand Firecracker microVMs, global | per-job microVMs behind an API |
| **Daytona / Northflank / Cloudflare Sandboxes** | Managed dev/code sandboxes | hosted dev environments |
| **AWS Lambda / Fargate** | Firecracker under the hood | event-driven short jobs |

**Tradeoff:** zero ops, but you don't control the kernel/TCB (defeats the "shrink what you
trust" thesis), and GPU options are limited/expensive.

---

## D. The GPU problem (read this if your AI projects need GPUs)

- **Firecracker has no GPU passthrough.** microVMs and GPUs don't mix today.
- **Strong isolation + GPU** means one of: a **full VM with VFIO passthrough** (Proxmox on a
  GPU box, one GPU per VM), or **containers + NVIDIA Container Toolkit** (weaker isolation,
  shared kernel + driver), or a **managed GPU service** (Modal, RunPod, Lambda Labs, etc.).
- Be honest: if you need GPUs *and* hostile-code isolation, you're choosing between
  "strong isolation, coarse GPU sharing (full VM)" and "fine GPU sharing, weak isolation
  (containers)." There's no free lunch yet.

---

## Recommendation matrix — pick by use case

| Use case | Recommended |
|---|---|
| **Untrusted AI-generated code / agents (CPU)** | **Kata** (easiest) or **Firecracker** on **rented bare metal** (Hetzner) — or **E2B** for zero ops. The sweet spot and the Axon thesis. |
| **GPU training/inference** | **Full VM + VFIO passthrough** (Proxmox), or **containers + nvidia runtime**, or **Modal/RunPod** managed. |
| **Per-project dev environments** | **Kata** (runs Docker images, hardware-isolated) on bare metal, or **Daytona/Northflank** managed. |
| **Max isolation / smallest TCB (research)** | The **`axon` nanovm** path — roadmap. Today: Firecracker + a minimal Linux guest. |
| **Cheapest, isolation not critical** | Plain **Docker** on any VPS. |

---

## Recommended setup (the one I'd actually build)

**Goal: run untrusted AI agents/code, strong isolation, runs anything, low ops, cheap.**

1. **Host:** a **Hetzner auction dedicated server** (confirm `/dev/kvm` exists). ~€35–50/mo.
2. **Isolation:** **Kata Containers** — hardware-isolated VMs that run *normal Docker/OCI
   images*, so your existing Python/AI project images Just Work but each runs in its own
   microVM, not a shared-kernel container. (Firecracker directly is leaner but means building
   guest images; Kata gets you 90% of the benefit with the container ergonomics you know.)
3. **Egress control:** put a **forward proxy / allowlist** in front of each sandbox so a
   project can only reach the endpoints you grant — the host-pinned `net:` story. (Default
   deny; allow `api.openai.com` etc. explicitly.)
4. **Policy + provenance:** apply the capability/effect policy. Today that's seccomp profiles
   derived per-project; with Axon, `axon-vm` derives the syscall allowlist + effect policy
   from the program's `.axmeta` so the policy is *derived from the code, can't drift*.
5. **Orchestration:** a thin daemon that spawns one sandbox per job from a queue and reaps it
   on exit (or use Fly.io Machines / E2B if you'd rather not run the orchestrator).

**Quick feasibility check on any candidate host:**
```bash
ls -l /dev/kvm                      # must exist
egrep -c '(vmx|svm)' /proc/cpuinfo  # >0 → hardware virt present
# Kata: https://github.com/kata-containers/kata-containers (kata-deploy on k8s, or kata + containerd)
# Firecracker: https://github.com/firecracker-microvm/firecracker
```

---

## How this maps to Axon

- **`axon-vm` is a Firecracker launcher with a capability/effect policy layer** (it derives
  seccomp + an effect policy from the `.axmeta` and boots a guest). So **"Axon for hosting AI
  projects" today = Firecracker microVM + Linux guest + `axon-vm` policy, on a bare-metal KVM
  host.** Real and usable now (see `AXON_VM.md`).
- **The "axon nanovm"** (`axon-guest-kernel`, unikernel-style: one program + a tiny verified
  kernel) is the **future** TCB-shrinking swap for the Linux guest. It **boots on real KVM and
  provably denies a syscall by policy** (`scripts/kernel_enforce_test.sh`), but it **cannot run
  general AI workloads yet** (no full program execution, no Python, no GPU). Roadmap in
  `AXON_ASI_KERNEL_DESIGN.md`.

**Bottom line:** rent **bare metal with KVM**, run **one Kata/Firecracker microVM per AI
project** with a **Linux guest + egress allowlist + capability policy** today, and treat the
**axon nanovm as the roadmap** for shrinking what you have to trust. If you want zero ops,
**E2B (CPU agents) or Modal (GPU)** are the managed shortcuts.
