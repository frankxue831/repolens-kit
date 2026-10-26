export interface Disposable {
    dispose(): void;
}

export interface Serializable extends Disposable {
    serialize(): string;
}

export type Config = {
    host: string;
    port: number;
};

export enum Status {
    Active = 'active',
    Inactive = 'inactive',
}

export { BaseService } from './base-service';
