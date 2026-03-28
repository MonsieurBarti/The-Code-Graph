import axios, { AxiosInstance, AxiosResponse } from 'axios';
import { EventEmitter } from 'events';
import * as path from 'path';
import * as fs from 'fs/promises';

export interface GraphNode {
  id: string;
  kind: 'function' | 'class' | 'module' | 'variable';
  label: string;
  filePath: string;
  line: number;
}

export interface GraphEdge {
  source: string;
  target: string;
  kind: 'imports' | 'calls' | 'extends' | 'implements';
}

export class GraphClient extends EventEmitter {
  private client: AxiosInstance;

  constructor(baseURL: string, timeout = 5000) {
    super();
    this.client = axios.create({ baseURL, timeout });
  }

  async getNode(id: string): Promise<GraphNode> {
    const res: AxiosResponse<GraphNode> = await this.client.get(`/nodes/${id}`);
    return res.data;
  }

  async listEdges(nodeId: string): Promise<GraphEdge[]> {
    const res: AxiosResponse<GraphEdge[]> = await this.client.get(`/nodes/${nodeId}/edges`);
    return res.data;
  }

  async importFile(filePath: string): Promise<void> {
    const abs = path.resolve(filePath);
    const content = await fs.readFile(abs, 'utf-8');
    await this.client.post('/import', { path: abs, content });
    this.emit('imported', abs);
  }

  async query(cypher: string): Promise<unknown[]> {
    const res = await this.client.post<unknown[]>('/query', { cypher });
    return res.data;
  }

  async deleteNode(id: string): Promise<void> {
    await this.client.delete(`/nodes/${id}`);
    this.emit('deleted', id);
  }
}
