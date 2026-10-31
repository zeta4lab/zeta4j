// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// BFS (Breadth-First Search) CUDA 커널
//
// GPU에서 레벨 동기화 BFS를 실행하기 위한 CUDA 커널입니다.
// CSR (Compressed Sparse Row) 포맷의 그래프를 사용합니다.

extern "C" {

/// BFS 레벨 확장 커널
///
/// 현재 프론티어의 모든 노드에서 이웃을 탐색하여 다음 프론티어를 생성합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열
/// @param distances 각 노드까지의 거리 (-1이면 미방문)
/// @param frontier 현재 프론티어 (1이면 현재 레벨, 0이면 아님)
/// @param next_frontier 다음 프론티어 (출력)
/// @param num_nodes 노드 수
/// @param current_level 현재 BFS 레벨
/// @param changed 변경 여부 플래그 (atomic)
__global__ void bfs_expand_frontier(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    int* distances,
    const int* frontier,
    int* next_frontier,
    const unsigned int num_nodes,
    const int current_level,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    // 현재 프론티어에 있는 노드만 처리
    if (frontier[node] != 1) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    // 모든 이웃 탐색
    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];

        // 미방문 노드인 경우 (atomic CAS 사용)
        int expected = -1;
        int old = atomicCAS(&distances[neighbor], expected, current_level + 1);

        if (old == -1) {
            // 성공적으로 거리를 설정함
            next_frontier[neighbor] = 1;
            *changed = 1;
        }
    }
}

/// BFS 프론티어 초기화 커널
///
/// 프론티어 배열을 0으로 초기화합니다.
///
/// @param frontier 프론티어 배열
/// @param num_nodes 노드 수
__global__ void bfs_clear_frontier(
    int* frontier,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    frontier[node] = 0;
}

/// 거리 배열 초기화 커널
///
/// 모든 거리를 -1 (미방문)로 초기화합니다.
///
/// @param distances 거리 배열
/// @param num_nodes 노드 수
__global__ void bfs_init_distances(
    int* distances,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    distances[node] = -1;
}

/// 시작 노드 설정 커널
///
/// @param distances 거리 배열
/// @param frontier 프론티어 배열
/// @param source 시작 노드 인덱스
__global__ void bfs_set_source(
    int* distances,
    int* frontier,
    const unsigned int source
) {
    if (threadIdx.x == 0 && blockIdx.x == 0) {
        distances[source] = 0;
        frontier[source] = 1;
    }
}

/// Bottom-Up BFS 커널 (대규모 프론티어용)
///
/// 미방문 노드가 프론티어에 있는 이웃을 가지고 있는지 확인합니다.
/// 프론티어가 클 때 더 효율적입니다.
///
/// @param row_offsets CSR 행 오프셋 (incoming edges)
/// @param col_indices CSR 열 인덱스 (incoming edges)
/// @param distances 거리 배열
/// @param frontier 현재 프론티어
/// @param next_frontier 다음 프론티어 (출력)
/// @param num_nodes 노드 수
/// @param current_level 현재 BFS 레벨
/// @param changed 변경 여부 플래그
__global__ void bfs_bottom_up(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    int* distances,
    const int* frontier,
    int* next_frontier,
    const unsigned int num_nodes,
    const int current_level,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    // 이미 방문한 노드는 스킵
    if (distances[node] != -1) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    // 프론티어에 있는 이웃이 있는지 확인
    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];

        if (frontier[neighbor] == 1) {
            // 이웃이 프론티어에 있으면 이 노드 방문
            distances[node] = current_level + 1;
            next_frontier[node] = 1;
            *changed = 1;
            return; // 하나만 찾으면 됨
        }
    }
}

/// 프론티어 크기 계산 커널 (부분합)
///
/// @param frontier 프론티어 배열
/// @param partial_counts 부분 카운트 (출력)
/// @param num_nodes 노드 수
__global__ void bfs_count_frontier(
    const int* frontier,
    unsigned int* partial_counts,
    const unsigned int num_nodes
) {
    extern __shared__ unsigned int sdata[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    unsigned int count = 0;
    if (node < num_nodes) {
        count = (frontier[node] == 1) ? 1 : 0;
    }
    sdata[tid] = count;
    __syncthreads();

    // 블록 내 리덕션
    for (unsigned int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            sdata[tid] += sdata[tid + s];
        }
        __syncthreads();
    }

    if (tid == 0) {
        partial_counts[blockIdx.x] = sdata[0];
    }
}

/// 경로 역추적을 위한 부모 기록 커널
///
/// BFS 확장 시 각 노드의 부모를 기록합니다.
///
/// @param row_offsets CSR 행 오프셋
/// @param col_indices CSR 열 인덱스
/// @param distances 거리 배열
/// @param parents 부모 노드 배열 (출력)
/// @param frontier 현재 프론티어
/// @param num_nodes 노드 수
/// @param current_level 현재 BFS 레벨
__global__ void bfs_expand_with_parents(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    int* distances,
    int* parents,
    const int* frontier,
    int* next_frontier,
    const unsigned int num_nodes,
    const int current_level,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    if (frontier[node] != 1) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];

        int expected = -1;
        int old = atomicCAS(&distances[neighbor], expected, current_level + 1);

        if (old == -1) {
            parents[neighbor] = (int)node;
            next_frontier[neighbor] = 1;
            *changed = 1;
        }
    }
}

/// SSSP (Single-Source Shortest Path) Bellman-Ford 스타일 커널
///
/// 가중치가 있는 그래프에서 최단 경로를 계산합니다.
///
/// @param row_offsets CSR 행 오프셋
/// @param col_indices CSR 열 인덱스
/// @param edge_weights 엣지 가중치
/// @param distances 거리 배열
/// @param num_nodes 노드 수
/// @param changed 변경 여부 플래그
__global__ void sssp_relax_edges(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    const float* edge_weights,
    float* distances,
    const unsigned int num_nodes,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    float node_dist = distances[node];
    if (node_dist == INFINITY) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];
        float weight = edge_weights[i];
        float new_dist = node_dist + weight;

        // Atomic min for float (using atomicCAS)
        float old = distances[neighbor];
        while (new_dist < old) {
            float assumed = old;
            old = __int_as_float(atomicCAS(
                (int*)&distances[neighbor],
                __float_as_int(assumed),
                __float_as_int(new_dist)
            ));
            if (old == assumed) {
                *changed = 1;
                break;
            }
        }
    }
}

/// SSSP 거리 초기화 커널
///
/// @param distances 거리 배열
/// @param source 시작 노드
/// @param num_nodes 노드 수
__global__ void sssp_init_distances(
    float* distances,
    const unsigned int source,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    distances[node] = (node == source) ? 0.0f : INFINITY;
}

} // extern "C"
