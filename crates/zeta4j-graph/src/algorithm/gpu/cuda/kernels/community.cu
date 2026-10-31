// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// Community Detection CUDA 커널
//
// GPU에서 커뮤니티 탐지 알고리즘을 실행하기 위한 CUDA 커널입니다.
// Label Propagation 및 관련 알고리즘을 포함합니다.

extern "C" {

/// Label Propagation 레이블 초기화 커널
///
/// 각 노드에 자신의 인덱스를 초기 레이블로 할당합니다.
///
/// @param labels 레이블 배열 (출력)
/// @param num_nodes 노드 수
__global__ void lpa_init_labels(
    unsigned int* labels,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    labels[node] = node;
}

/// Label Propagation 레이블 업데이트 커널 (동기식)
///
/// 각 노드에 대해 이웃들의 가장 빈번한 레이블로 업데이트합니다.
/// Atomic 연산을 사용하여 레이블 카운트를 집계합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열
/// @param labels 현재 레이블 배열
/// @param new_labels 새 레이블 배열 (출력)
/// @param num_nodes 노드 수
/// @param changed 변경 여부 플래그 (atomic)
__global__ void lpa_update_labels(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    const unsigned int* labels,
    unsigned int* new_labels,
    const unsigned int num_nodes,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    // 이웃이 없으면 현재 레이블 유지
    if (start >= end) {
        new_labels[node] = labels[node];
        return;
    }

    // 로컬 레이블 카운트 (작은 해시 테이블 사용)
    // GPU에서는 제한된 메모리 사용을 위해 작은 해시 테이블 사용
    const int HASH_SIZE = 32;
    unsigned int local_labels[HASH_SIZE];
    unsigned int local_counts[HASH_SIZE];

    for (int i = 0; i < HASH_SIZE; i++) {
        local_labels[i] = 0xFFFFFFFF;  // 무효값
        local_counts[i] = 0;
    }

    // 이웃들의 레이블 카운트
    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];
        unsigned int neighbor_label = labels[neighbor];

        // 간단한 해시로 슬롯 찾기
        unsigned int slot = neighbor_label % HASH_SIZE;
        unsigned int original_slot = slot;

        // 선형 프로빙
        while (true) {
            if (local_labels[slot] == neighbor_label) {
                // 기존 레이블 카운트 증가
                local_counts[slot]++;
                break;
            } else if (local_labels[slot] == 0xFFFFFFFF) {
                // 새 레이블 추가
                local_labels[slot] = neighbor_label;
                local_counts[slot] = 1;
                break;
            }
            slot = (slot + 1) % HASH_SIZE;
            if (slot == original_slot) break; // 해시 테이블 가득 참
        }
    }

    // 가장 빈번한 레이블 찾기 (동률 시 작은 레이블 선택)
    unsigned int max_count = 0;
    unsigned int best_label = labels[node]; // 기본값은 현재 레이블

    for (int i = 0; i < HASH_SIZE; i++) {
        if (local_labels[i] != 0xFFFFFFFF) {
            if (local_counts[i] > max_count ||
                (local_counts[i] == max_count && local_labels[i] < best_label)) {
                max_count = local_counts[i];
                best_label = local_labels[i];
            }
        }
    }

    new_labels[node] = best_label;

    // 레이블 변경 감지
    if (labels[node] != best_label) {
        *changed = 1;
    }
}

/// Label Propagation 레이블 복사 커널
///
/// 새 레이블을 현재 레이블로 복사합니다.
///
/// @param labels 현재 레이블 배열
/// @param new_labels 새 레이블 배열
/// @param num_nodes 노드 수
__global__ void lpa_copy_labels(
    unsigned int* labels,
    const unsigned int* new_labels,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    labels[node] = new_labels[node];
}

/// 커뮤니티 수 계산 커널 (부분합)
///
/// 고유한 레이블 수를 계산합니다.
///
/// @param labels 레이블 배열
/// @param label_exists 레이블 존재 여부 배열 (크기: num_nodes)
/// @param num_nodes 노드 수
__global__ void lpa_mark_labels(
    const unsigned int* labels,
    int* label_exists,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    // 레이블을 인덱스로 사용하여 존재 표시
    unsigned int label = labels[node];
    if (label < num_nodes) {
        label_exists[label] = 1;
    }
}

/// 고유 레이블 카운트 커널
///
/// @param label_exists 레이블 존재 여부 배열
/// @param partial_counts 부분 카운트 (출력)
/// @param num_nodes 노드 수
__global__ void lpa_count_labels(
    const int* label_exists,
    unsigned int* partial_counts,
    const unsigned int num_nodes
) {
    extern __shared__ unsigned int sdata[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    unsigned int count = 0;
    if (node < num_nodes && label_exists[node] == 1) {
        count = 1;
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

/// 가중치 기반 Label Propagation 업데이트 커널
///
/// 엣지 가중치를 고려하여 레이블을 업데이트합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열
/// @param edge_weights 엣지 가중치 배열
/// @param labels 현재 레이블 배열
/// @param new_labels 새 레이블 배열 (출력)
/// @param num_nodes 노드 수
/// @param changed 변경 여부 플래그 (atomic)
__global__ void lpa_update_labels_weighted(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    const float* edge_weights,
    const unsigned int* labels,
    unsigned int* new_labels,
    const unsigned int num_nodes,
    int* changed
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    if (start >= end) {
        new_labels[node] = labels[node];
        return;
    }

    // 로컬 레이블 가중치 (작은 해시 테이블)
    const int HASH_SIZE = 32;
    unsigned int local_labels[HASH_SIZE];
    float local_weights[HASH_SIZE];

    for (int i = 0; i < HASH_SIZE; i++) {
        local_labels[i] = 0xFFFFFFFF;
        local_weights[i] = 0.0f;
    }

    // 이웃들의 레이블 가중치 합산
    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];
        unsigned int neighbor_label = labels[neighbor];
        float weight = edge_weights[i];

        unsigned int slot = neighbor_label % HASH_SIZE;
        unsigned int original_slot = slot;

        while (true) {
            if (local_labels[slot] == neighbor_label) {
                local_weights[slot] += weight;
                break;
            } else if (local_labels[slot] == 0xFFFFFFFF) {
                local_labels[slot] = neighbor_label;
                local_weights[slot] = weight;
                break;
            }
            slot = (slot + 1) % HASH_SIZE;
            if (slot == original_slot) break;
        }
    }

    // 가장 높은 가중치의 레이블 찾기
    float max_weight = 0.0f;
    unsigned int best_label = labels[node];

    for (int i = 0; i < HASH_SIZE; i++) {
        if (local_labels[i] != 0xFFFFFFFF) {
            if (local_weights[i] > max_weight ||
                (local_weights[i] == max_weight && local_labels[i] < best_label)) {
                max_weight = local_weights[i];
                best_label = local_labels[i];
            }
        }
    }

    new_labels[node] = best_label;

    if (labels[node] != best_label) {
        *changed = 1;
    }
}

/// Triangle Counting 커널 (노드별 삼각형 수)
///
/// 각 노드가 속한 삼각형 개수를 계산합니다.
/// 효율적인 교집합 연산을 위해 정렬된 이웃 목록을 가정합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열 (정렬됨)
/// @param triangle_counts 노드별 삼각형 카운트 (출력)
/// @param num_nodes 노드 수
__global__ void triangle_count_per_node(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    unsigned long long* triangle_counts,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    unsigned int start_u = row_offsets[node];
    unsigned int end_u = row_offsets[node + 1];

    unsigned long long count = 0;

    // node의 모든 이웃 v에 대해
    for (unsigned int i = start_u; i < end_u; i++) {
        unsigned int v = col_indices[i];

        // 더 큰 노드만 고려 (중복 카운트 방지)
        if (v <= node) continue;

        unsigned int start_v = row_offsets[v];
        unsigned int end_v = row_offsets[v + 1];

        // node의 이웃과 v의 이웃의 교집합 계산
        unsigned int ptr_u = i + 1;  // node의 다음 이웃부터
        unsigned int ptr_v = start_v;

        while (ptr_u < end_u && ptr_v < end_v) {
            unsigned int neighbor_u = col_indices[ptr_u];
            unsigned int neighbor_v = col_indices[ptr_v];

            // node보다 큰 이웃만 고려
            if (neighbor_u <= node) {
                ptr_u++;
                continue;
            }
            if (neighbor_v <= node) {
                ptr_v++;
                continue;
            }

            if (neighbor_u == neighbor_v) {
                // 삼각형 발견 (node, v, neighbor_u)
                count++;
                ptr_u++;
                ptr_v++;
            } else if (neighbor_u < neighbor_v) {
                ptr_u++;
            } else {
                ptr_v++;
            }
        }
    }

    triangle_counts[node] = count;
}

/// Triangle Counting 전체 합산 커널
///
/// @param triangle_counts 노드별 삼각형 카운트
/// @param partial_sums 부분합 (출력)
/// @param num_nodes 노드 수
__global__ void triangle_count_reduce(
    const unsigned long long* triangle_counts,
    unsigned long long* partial_sums,
    const unsigned int num_nodes
) {
    extern __shared__ unsigned long long sdata_ull[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    sdata_ull[tid] = (node < num_nodes) ? triangle_counts[node] : 0;
    __syncthreads();

    // 블록 내 리덕션
    for (unsigned int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            sdata_ull[tid] += sdata_ull[tid + s];
        }
        __syncthreads();
    }

    if (tid == 0) {
        partial_sums[blockIdx.x] = sdata_ull[0];
    }
}

/// 정렬된 이웃 목록에서 이진 검색
///
/// @param col_indices CSR 열 인덱스 배열
/// @param start 검색 시작 위치
/// @param end 검색 끝 위치
/// @param target 검색할 노드
/// @return 찾으면 true, 아니면 false
__device__ bool binary_search_neighbor(
    const unsigned int* col_indices,
    unsigned int start,
    unsigned int end,
    unsigned int target
) {
    while (start < end) {
        unsigned int mid = start + (end - start) / 2;
        unsigned int val = col_indices[mid];
        if (val == target) return true;
        if (val < target) start = mid + 1;
        else end = mid;
    }
    return false;
}

/// Triangle Counting 최적화 커널 (엣지 기반)
///
/// 각 엣지에 대해 공통 이웃을 찾아 삼각형을 계산합니다.
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param col_indices CSR 열 인덱스 배열 (정렬됨)
/// @param edge_triangle_counts 엣지별 삼각형 카운트 (출력)
/// @param num_edges 엣지 수
/// @param num_nodes 노드 수
__global__ void triangle_count_per_edge(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    unsigned int* edge_triangle_counts,
    const unsigned int num_edges,
    const unsigned int num_nodes
) {
    unsigned int edge_idx = blockIdx.x * blockDim.x + threadIdx.x;
    if (edge_idx >= num_edges) return;

    // 엣지 (u, v) 찾기
    // row_offsets에서 이진 검색으로 u 찾기
    unsigned int u = 0;
    unsigned int lo = 0, hi = num_nodes;
    while (lo < hi) {
        unsigned int mid = lo + (hi - lo) / 2;
        if (row_offsets[mid + 1] <= edge_idx) {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    u = lo;

    unsigned int v = col_indices[edge_idx];

    // u < v인 엣지만 처리 (중복 방지)
    if (u >= v) {
        edge_triangle_counts[edge_idx] = 0;
        return;
    }

    unsigned int start_u = row_offsets[u];
    unsigned int end_u = row_offsets[u + 1];
    unsigned int start_v = row_offsets[v];
    unsigned int end_v = row_offsets[v + 1];

    // 교집합 계산 (정렬된 리스트 머지)
    unsigned int count = 0;
    unsigned int ptr_u = start_u;
    unsigned int ptr_v = start_v;

    while (ptr_u < end_u && ptr_v < end_v) {
        unsigned int nu = col_indices[ptr_u];
        unsigned int nv = col_indices[ptr_v];

        if (nu == nv) {
            // 공통 이웃 발견 -> 삼각형
            count++;
            ptr_u++;
            ptr_v++;
        } else if (nu < nv) {
            ptr_u++;
        } else {
            ptr_v++;
        }
    }

    edge_triangle_counts[edge_idx] = count;
}

/// Clustering Coefficient 계산 커널
///
/// 노드의 클러스터링 계수를 계산합니다.
/// CC = 2 * triangles / (degree * (degree - 1))
///
/// @param row_offsets CSR 행 오프셋 배열
/// @param triangle_counts 노드별 삼각형 카운트
/// @param clustering_coeffs 클러스터링 계수 (출력)
/// @param num_nodes 노드 수
__global__ void compute_clustering_coefficient(
    const unsigned int* row_offsets,
    const unsigned long long* triangle_counts,
    float* clustering_coeffs,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    unsigned int degree = row_offsets[node + 1] - row_offsets[node];

    if (degree < 2) {
        clustering_coeffs[node] = 0.0f;
        return;
    }

    unsigned long long triangles = triangle_counts[node];
    unsigned long long possible_triangles = (unsigned long long)degree * (degree - 1) / 2;

    clustering_coeffs[node] = (float)triangles / (float)possible_triangles;
}

} // extern "C"
