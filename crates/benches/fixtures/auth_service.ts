import { Injectable } from '@angular/core';
import { HttpClient, HttpHeaders } from '@angular/common/http';
import { Observable, throwError } from 'rxjs';
import { catchError, map } from 'rxjs/operators';

export interface AuthToken {
  accessToken: string;
  refreshToken: string;
  expiresIn: number;
}

export interface User {
  id: string;
  email: string;
  roles: string[];
}

@Injectable({ providedIn: 'root' })
export class AuthService {
  private readonly baseUrl = '/api/auth';

  constructor(private http: HttpClient) {}

  login(email: string, password: string): Observable<AuthToken> {
    return this.http
      .post<AuthToken>(`${this.baseUrl}/login`, { email, password })
      .pipe(catchError(this.handleError));
  }

  logout(): Observable<void> {
    const headers = this.buildHeaders();
    return this.http.post<void>(`${this.baseUrl}/logout`, {}, { headers });
  }

  refreshToken(token: string): Observable<AuthToken> {
    return this.http
      .post<AuthToken>(`${this.baseUrl}/refresh`, { token })
      .pipe(map((res) => res), catchError(this.handleError));
  }

  getCurrentUser(): Observable<User> {
    const headers = this.buildHeaders();
    return this.http.get<User>(`${this.baseUrl}/me`, { headers });
  }

  private buildHeaders(): HttpHeaders {
    const token = localStorage.getItem('access_token') ?? '';
    return new HttpHeaders({ Authorization: `Bearer ${token}` });
  }

  private handleError(error: unknown): Observable<never> {
    console.error('AuthService error', error);
    return throwError(() => new Error('Authentication failed'));
  }
}
