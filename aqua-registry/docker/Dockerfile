FROM golang:1.25.0-bookworm
WORKDIR /workspace
ENV AQUA_LOG_COLOR=always
ENV AQUA_POLICY_CONFIG=/workspace/aqua-policy.yaml
ENV PATH=/root/.local/share/aquaproj-aqua/bin:/root/.cargo/bin:$PATH
SHELL ["/bin/bash", "-o", "pipefail", "-c"]
RUN \
  apt-get update && \
  apt-get install --no-install-recommends -y tree && \
  apt-get clean && \
  rm -rf /var/lib/apt/lists/*
RUN curl -sSfL -O https://raw.githubusercontent.com/aquaproj/aqua-installer/v4.0.3/aqua-installer
RUN curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --default-toolchain stable --profile minimal
RUN echo "d76bb5c3a23025e9d344aeba04c0a4d4ed6c9fb05e85530c8c3be2ef8e8f3a6a  aqua-installer" | sha256sum -c -
RUN chmod +x aqua-installer
RUN ./aqua-installer -v v2.53.11
COPY aqua-test.yaml aqua.yaml
COPY aqua-policy.yaml aqua-policy.yaml
