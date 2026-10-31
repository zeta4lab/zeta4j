// Copyright 2026 Kangyou Choi
// SPDX-License-Identifier: Apache-2.0

// PageRank CUDA 커널
//
// GPU에서 PageRank 알고리즘을 실행하기 위한 CUDA 커널입니다.
// CSR (Compressed Sparse Row) 포맷의 그래프를 사용합니다.

extern "C" {

/// PageRank 반복 계산 커널
///
/// 각 노드의 새로운 PageRank 점수를 계산합니다.
/// new_score[i] = teleport + damping * sum(old_score[j] / out_degree[j]) for j in incoming[i]
///
/// @param row_offsets CSR 행 오프셋 배열 (길이: num_nodes + 1)
/// @param col_indices CSR 열 인덱스 배열 (들어오는 엣지의 소스 노드)
/// @param out_degrees 각 노드의 나가는 차수
/// @param old_scores 이전 반복의 점수
/// @param new_scores 새로운 점수 (출력)
/// @param num_nodes 노드 수
/// @param damping 감쇠 계수 (보통 0.85)
/// @param teleport 텔레포트 값 ((1 - damping) / num_nodes)
__global__ void pagerank_iteration(
    const unsigned int* row_offsets,
    const unsigned int* col_indices,
    const unsigned int* out_degrees,
    const float* old_scores,
    float* new_scores,
    const unsigned int num_nodes,
    const float damping,
    const float teleport
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    float sum = 0.0f;
    unsigned int start = row_offsets[node];
    unsigned int end = row_offsets[node + 1];

    // 들어오는 엣지로부터 점수 수집
    for (unsigned int i = start; i < end; i++) {
        unsigned int neighbor = col_indices[i];
        unsigned int degree = out_degrees[neighbor];
        if (degree > 0) {
            sum += old_scores[neighbor] / (float)degree;
        }
    }

    new_scores[node] = teleport + damping * sum;
}

/// 댕글링 노드 기여도 계산 커널
///
/// 나가는 엣지가 없는 노드들의 점수 합계를 계산합니다.
///
/// @param out_degrees 각 노드의 나가는 차수
/// @param scores 현재 점수
/// @param partial_sums 부분합 (출력, 블록당 하나)
/// @param num_nodes 노드 수
__global__ void compute_dangling_sum(
    const unsigned int* out_degrees,
    const float* scores,
    float* partial_sums,
    const unsigned int num_nodes
) {
    extern __shared__ float sdata[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    // 각 스레드가 자신의 노드 처리
    float val = 0.0f;
    if (node < num_nodes && out_degrees[node] == 0) {
        val = scores[node];
    }
    sdata[tid] = val;
    __syncthreads();

    // 블록 내 리덕션
    for (unsigned int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            sdata[tid] += sdata[tid + s];
        }
        __syncthreads();
    }

    // 블록 대표값 저장
    if (tid == 0) {
        partial_sums[blockIdx.x] = sdata[0];
    }
}

/// 댕글링 기여도 추가 커널
///
/// 모든 노드에 댕글링 노드 기여도를 추가합니다.
///
/// @param scores 현재 점수 (in-place 수정)
/// @param dangling_contribution 댕글링 기여도 (damping * dangling_sum / num_nodes)
/// @param num_nodes 노드 수
__global__ void add_dangling_contribution(
    float* scores,
    const float dangling_contribution,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    scores[node] += dangling_contribution;
}

/// 수렴 검사 커널 (최대 차이 계산)
///
/// 이전 점수와 새 점수의 최대 차이를 계산합니다.
///
/// @param old_scores 이전 점수
/// @param new_scores 새 점수
/// @param max_diffs 블록별 최대 차이 (출력)
/// @param num_nodes 노드 수
__global__ void compute_max_diff(
    const float* old_scores,
    const float* new_scores,
    float* max_diffs,
    const unsigned int num_nodes
) {
    extern __shared__ float sdata[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    // 각 스레드가 자신의 노드 처리
    float diff = 0.0f;
    if (node < num_nodes) {
        diff = fabsf(new_scores[node] - old_scores[node]);
    }
    sdata[tid] = diff;
    __syncthreads();

    // 블록 내 최대값 리덕션
    for (unsigned int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            sdata[tid] = fmaxf(sdata[tid], sdata[tid + s]);
        }
        __syncthreads();
    }

    // 블록 대표값 저장
    if (tid == 0) {
        max_diffs[blockIdx.x] = sdata[0];
    }
}

/// 점수 정규화 커널
///
/// 모든 점수를 총합으로 나누어 정규화합니다.
///
/// @param scores 점수 배열 (in-place 수정)
/// @param total 점수 총합
/// @param num_nodes 노드 수
__global__ void normalize_scores(
    float* scores,
    const float total,
    const unsigned int num_nodes
) {
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;
    if (node >= num_nodes) return;

    if (total > 0.0f) {
        scores[node] /= total;
    }
}

/// 합계 계산 커널 (정규화용)
///
/// @param scores 점수 배열
/// @param partial_sums 부분합 (출력)
/// @param num_nodes 노드 수
__global__ void compute_sum(
    const float* scores,
    float* partial_sums,
    const unsigned int num_nodes
) {
    extern __shared__ float sdata[];

    unsigned int tid = threadIdx.x;
    unsigned int node = blockIdx.x * blockDim.x + threadIdx.x;

    float val = 0.0f;
    if (node < num_nodes) {
        val = scores[node];
    }
    sdata[tid] = val;
    __syncthreads();

    // 블록 내 리덕션
    for (unsigned int s = blockDim.x / 2; s > 0; s >>= 1) {
        if (tid < s) {
            sdata[tid] += sdata[tid + s];
        }
        __syncthreads();
    }

    if (tid == 0) {
        partial_sums[blockIdx.x] = sdata[0];
    }
}

} // extern "C"
