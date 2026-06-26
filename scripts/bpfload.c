// R23 — Axon eBPF loader + verifier harness.
//
// `/usr/sbin/bpftool` is a kernel-version wrapper that cannot find its
// per-kernel binary under this WSL2 kernel (6.6.x-microsoft-WSL2) and silently
// no-ops, so this tiny loader does a genuine in-kernel verifier round-trip
// instead: it reads an ELF .bpf.o, creates any maps declared in the `maps`
// section, resolves the `R_BPF_64_64` map relocation (patches the `lddw`
// immediate with the map fd and sets src_reg = BPF_PSEUDO_MAP_FD), then issues
// `bpf(BPF_PROG_LOAD)`. Exit 0 iff the in-kernel verifier ACCEPTS the program.
//
// Usage:  bpfload <file.bpf.o> <section> [prog_type_int]
//   prog_type defaults to BPF_PROG_TYPE_SOCKET_FILTER (1).
//
// Build:  cc scripts/bpfload.c -o <out>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <linux/bpf.h>
#include <sys/syscall.h>
#include <elf.h>
#include <errno.h>

#ifndef BPF_PSEUDO_MAP_FD
#define BPF_PSEUDO_MAP_FD 1
#endif

struct map_def { unsigned int type, key_size, value_size, max_entries, flags; };

static long sys_bpf(int cmd, union bpf_attr *a, unsigned s){ return syscall(__NR_bpf,cmd,a,s); }

int main(int argc,char**argv){
  if(argc<3){fprintf(stderr,"usage: %s file.o section [prog_type]\n",argv[0]);return 2;}
  const char*sec=argv[2];
  int prog_type=(argc>3)?atoi(argv[3]):BPF_PROG_TYPE_SOCKET_FILTER;
  FILE*f=fopen(argv[1],"rb"); if(!f){perror("open");return 2;}
  fseek(f,0,SEEK_END); long n=ftell(f); fseek(f,0,SEEK_SET);
  unsigned char*buf=malloc(n); if(fread(buf,1,n,f)!=(size_t)n)return 2; fclose(f);

  Elf64_Ehdr*eh=(Elf64_Ehdr*)buf;
  Elf64_Shdr*sh=(Elf64_Shdr*)(buf+eh->e_shoff);
  char*shstr=(char*)(buf+sh[eh->e_shstrndx].sh_offset);
  int prog_si=-1, maps_si=-1, sym_si=-1, rel_si=-1;
  for(int i=0;i<eh->e_shnum;i++){
    const char*nm=shstr+sh[i].sh_name;
    if(!strcmp(nm,sec)) prog_si=i;
    else if(!strcmp(nm,"maps")) maps_si=i;
    else if(sh[i].sh_type==SHT_SYMTAB) sym_si=i;
  }
  if(prog_si<0){fprintf(stderr,"section %s not found\n",sec);return 2;}
  // find the rel section that targets prog_si
  for(int i=0;i<eh->e_shnum;i++) if(sh[i].sh_type==SHT_REL && (int)sh[i].sh_info==prog_si) rel_si=i;

  struct bpf_insn*insns=(struct bpf_insn*)(buf+sh[prog_si].sh_offset);
  unsigned long insn_cnt=sh[prog_si].sh_size/8;

  // Create maps: one fd per symbol that lives in the maps section.
  int map_fd_for_sym[4096]; for(int i=0;i<4096;i++) map_fd_for_sym[i]=-1;
  if(sym_si>=0 && maps_si>=0){
    Elf64_Sym*syms=(Elf64_Sym*)(buf+sh[sym_si].sh_offset);
    int nsym=sh[sym_si].sh_size/sizeof(Elf64_Sym);
    char*symstr=(char*)(buf+sh[sh[sym_si].sh_link].sh_offset);
    unsigned char*mapsdata=buf+sh[maps_si].sh_offset;
    for(int s=0;s<nsym;s++){
      if(syms[s].st_shndx==maps_si){
        struct map_def*md=(struct map_def*)(mapsdata+syms[s].st_value);
        union bpf_attr ma; memset(&ma,0,sizeof(ma));
        ma.map_type=md->type; ma.key_size=md->key_size;
        ma.value_size=md->value_size; ma.max_entries=md->max_entries;
        ma.map_flags=md->flags;
        int mfd=sys_bpf(BPF_MAP_CREATE,&ma,sizeof(ma));
        if(mfd<0){fprintf(stderr,"map create '%s' failed: %s\n",symstr+syms[s].st_name,strerror(errno));return 2;}
        map_fd_for_sym[s]=mfd;
        fprintf(stderr,"created map '%s' fd=%d (type=%u k=%u v=%u max=%u)\n",
          symstr+syms[s].st_name,mfd,md->type,md->key_size,md->value_size,md->max_entries);
      }
    }
  }
  // Apply R_BPF_64_64 relocations: patch lddw with PSEUDO_MAP_FD + map fd.
  if(rel_si>=0 && sym_si>=0){
    Elf64_Rel*rels=(Elf64_Rel*)(buf+sh[rel_si].sh_offset);
    int nrel=sh[rel_si].sh_size/sizeof(Elf64_Rel);
    Elf64_Sym*syms=(Elf64_Sym*)(buf+sh[sym_si].sh_offset);
    for(int r=0;r<nrel;r++){
      int symidx=ELF64_R_SYM(rels[r].r_info);
      int idx=rels[r].r_offset/8;
      int fd=map_fd_for_sym[symidx];
      if(fd>=0){
        insns[idx].src_reg = BPF_PSEUDO_MAP_FD;
        insns[idx].imm = fd;
      }
    }
  }

  char log[1<<18]={0};
  union bpf_attr attr; memset(&attr,0,sizeof(attr));
  attr.prog_type=prog_type; attr.insn_cnt=insn_cnt;
  attr.insns=(unsigned long)insns; attr.license=(unsigned long)"GPL";
  attr.log_level=1; attr.log_buf=(unsigned long)log; attr.log_size=sizeof(log);
  int fd=sys_bpf(BPF_PROG_LOAD,&attr,sizeof(attr));
  if(fd<0){fprintf(stderr,"BPF_PROG_LOAD failed: %s\nverifier log:\n%s\n",strerror(errno),log);return 1;}
  fprintf(stderr,"VERIFIER ACCEPTED: prog fd=%d insns=%lu\n%s\n",fd,(unsigned long)insn_cnt,log);
  close(fd); return 0;
}
