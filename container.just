import 'git.just'

[no-cd]
build image=git_repo_name:
  podman build --rm --layers -t {{ image }}:{{ git_tag }}-{{ arch }} --build-arg TAG={{ git_tag }} --build-arg ARCH={{ arch }} --platform linux/{{ arch }} .

# Scan image using grype
scan image=git_repo_name:
  grype podman:localhost/{{ image }}:{{ git_tag }}-{{ arch }}

# log into AWS ECR public
ecr-login registry="public.ecr.aws/zero-downtime":  
  aws ecr-public get-login-password --region us-east-1 | podman login --username AWS --password-stdin {{ registry }}

# create new AWS ECR public repository
create-public-repo image=git_repo_name:
  aws ecr-public create-repository --repository-name {{ image }} --region us-east-1

# tag and push all actual images and multi-arch manifest
push image=git_repo_name registry="public.ecr.aws/zero-downtime" extra_tags="": ecr-login
  #!/usr/bin/env bash
  for t in {{ git_tag }} latest {{ extra_tags }}; do
    echo "Tagging image with {{ registry }}/{{ image }}:${t}-{{ arch }}"
    podman tag {{ image }}:{{ git_tag }}-{{ arch }} {{ registry }}/{{ image }}:${t}-{{ arch }}
    podman manifest rm {{ image }}:$t || true
    podman manifest create {{ image }}:$t
    for a in {{ all_archs }}; do
      podman image exists {{ registry }}/{{ image }}:$t-$a && \
      podman manifest add {{ image }}:$t containers-storage:{{ registry }}/{{ image }}:$t-$a
    done
    echo "Pushing manifest {{ image }}:$t"
    podman manifest push --all {{ image }}:$t docker://{{ registry }}/{{ image }}:$t
  done

# cleanup local images
clean image=git_repo_name extra_tags="":
  #!/usr/bin/env bash
  for t in {{ git_tag }} latest {{ extra_tags }}; do
    for a in {{ all_archs }}; do
      podman image exists {{ image }}:$t-$a && podman image rm -f {{ image }}:$t-$a || true
    done
  done

# delete all remote untagged and in-dev images, keep 10 tagged
[no-cd]
rm-remote-untagged image=git_repo_name:
  @echo "Removing all untagged and in-dev images from {{ image }}"
  .ci/ecr_public_lifecycle.py --repo {{ image }} --dev
